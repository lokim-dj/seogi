//! Google, for the one Drive folder the phone and this computer share.
//!
//! Seogi asks for one permission, `drive.file`: it sees only the files and
//! folders a Seogi app made (the `서기` folder and what is in it), nothing
//! else in the user's Drive. Signing in happens in the user's
//! own browser, which sends the answer back to this computer only
//! (`127.0.0.1`, a port chosen for that one sign-in, with PKCE).
//!
//! The OAuth client is put in at build time (`SEOGI_GOOGLE_CLIENT_ID`,
//! `SEOGI_GOOGLE_CLIENT_SECRET`); it is never in the repository. For an
//! installed app Google does not treat the secret as secret, but it still
//! stays out of the public sources. A build without them simply cannot
//! connect, and says so.
//!
//! The refresh token is kept in `google.json` in the app's data folder,
//! readable by the user only. Tokens never go to the log.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// The one permission asked for: files a Seogi app made.
pub const SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
/// The folder the phone and this computer share. Found by its marker, not
/// its name.
pub const FOLDER_NAME: &str = "서기";
/// The name the folder had before this computer put its own meetings in
/// it too: a folder still named so is renamed to [`FOLDER_NAME`] (the same
/// folder; a name the user chose is left alone).
pub const OLD_FOLDER_NAME: &str = "서기 폰 녹음";
/// The marker on a meeting's folder inside the shared one: its value is
/// the meeting's id on this computer.
pub const MEETING_KEY: &str = "seogi_meeting";
/// The marker every Seogi app looks for (the phone, the Mac and Windows).
pub const MARK_KEY: &str = "seogi";
pub const MARK_VALUE: &str = "recordings-root";
pub const FOLDER_MIME: &str = "application/vnd.google-apps.folder";

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
const API: &str = "https://www.googleapis.com/drive/v3";
const UPLOAD_API: &str = "https://www.googleapis.com/upload/drive/v3";
const TOKENS_NAME: &str = "google.json";
/// How long the browser has to come back with an answer.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// The OAuth client this build was made with.
#[derive(Clone, Copy)]
struct Client {
    id: &'static str,
    secret: &'static str,
}

fn client() -> Option<Client> {
    let id = option_env!("SEOGI_GOOGLE_CLIENT_ID").unwrap_or("").trim();
    let secret = option_env!("SEOGI_GOOGLE_CLIENT_SECRET")
        .unwrap_or("")
        .trim();
    (!id.is_empty() && !secret.is_empty()).then_some(Client { id, secret })
}

/// Whether this build can connect to Google at all.
pub fn available() -> bool {
    client().is_some()
}

/// The sign-in no longer works (revoked, expired, or the permission was
/// taken back): the user has to connect again.
#[derive(Debug)]
pub struct NeedsReconnect;

impl std::fmt::Display for NeedsReconnect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Google needs to be connected again")
    }
}

impl std::error::Error for NeedsReconnect {}

/// Google could not be reached (no connection, a timeout, a broken answer).
#[derive(Debug)]
pub struct Offline(pub String);

impl std::fmt::Display for Offline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Google could not be reached ({})", self.0)
    }
}

impl std::error::Error for Offline {}

/// Drive answered with a status the call did not expect.
#[derive(Debug)]
pub struct HttpStatus(pub u16);

impl std::fmt::Display for HttpStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Google answered {}", self.0)
    }
}

impl std::error::Error for HttpStatus {}

/// What is kept on disk.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
struct Saved {
    refresh_token: String,
    /// The account's address, as Drive gave it.
    #[serde(default)]
    account: String,
    /// Google refused the refresh token: kept to name the account when
    /// asking the user to connect again.
    #[serde(default)]
    lost: bool,
}

/// A connection to the user's Google account.
pub struct Google {
    data_dir: PathBuf,
    saved: Mutex<Option<Saved>>,
    access: Mutex<Option<(String, Instant)>>,
    connecting: AtomicBool,
    cancel: AtomicBool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Where the account stands, for the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    NotConnected,
    Connected { account: String },
    Lost { account: String },
}

impl Google {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            data_dir: data_dir.to_path_buf(),
            saved: Mutex::new(load(data_dir)),
            access: Mutex::new(None),
            connecting: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
        }
    }

    pub fn standing(&self) -> Standing {
        match lock(&self.saved).as_ref() {
            None => Standing::NotConnected,
            Some(saved) if saved.lost => Standing::Lost {
                account: saved.account.clone(),
            },
            Some(saved) => Standing::Connected {
                account: saved.account.clone(),
            },
        }
    }

    pub fn is_connecting(&self) -> bool {
        self.connecting.load(Ordering::SeqCst)
    }

    /// Signs in in the browser (`open` opens the address) and waits for the
    /// answer. Blocks until it comes, the user gives up, or five minutes
    /// pass. Returns the account's address.
    pub fn connect(&self, open: impl FnOnce(&str) -> Result<()>) -> Result<String> {
        let client = client().ok_or_else(|| anyhow!("unavailable"))?;
        if self.connecting.swap(true, Ordering::SeqCst) {
            bail!("already_connecting");
        }
        self.cancel.store(false, Ordering::SeqCst);
        let result = self.connect_with(client, open);
        self.connecting.store(false, Ordering::SeqCst);
        result
    }

    fn connect_with(
        &self,
        client: Client,
        open: impl FnOnce(&str) -> Result<()>,
    ) -> Result<String> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .context("could not listen for the browser's answer")?;
        let port = listener.local_addr()?.port();
        let redirect = format!("http://127.0.0.1:{port}");
        let verifier = random_text(64)?;
        let state = random_text(32)?;
        open(&auth_url(
            client.id,
            &redirect,
            &challenge(&verifier),
            &state,
        ))?;
        let code = wait_for_code(&listener, &state, &self.cancel, CONNECT_TIMEOUT)?;
        let tokens = tauri::async_runtime::block_on(async {
            let response = http()?
                .post(TOKEN_URL)
                .form(&[
                    ("code", code.as_str()),
                    ("client_id", client.id),
                    ("client_secret", client.secret),
                    ("redirect_uri", redirect.as_str()),
                    ("grant_type", "authorization_code"),
                    ("code_verifier", verifier.as_str()),
                ])
                .send()
                .await
                .map_err(|e| anyhow!("Google could not be reached ({})", e.without_url()))?;
            let status = response.status();
            let body: Value = response.json().await.unwrap_or(Value::Null);
            if !status.is_success() {
                bail!(
                    "Google refused the sign-in ({status}: {})",
                    body["error"].as_str().unwrap_or("")
                );
            }
            Ok(body)
        })?;
        let refresh = tokens["refresh_token"]
            .as_str()
            .filter(|token| !token.is_empty())
            .ok_or_else(|| anyhow!("Google gave no lasting sign-in"))?
            .to_string();
        let access = tokens["access_token"].as_str().unwrap_or("").to_string();
        let lasts = tokens["expires_in"].as_u64().unwrap_or(3_000);
        let account = account_of(&access).unwrap_or_default();
        let saved = Saved {
            refresh_token: refresh,
            account: account.clone(),
            lost: false,
        };
        save(&self.data_dir, &saved)?;
        *lock(&self.saved) = Some(saved);
        *lock(&self.access) = Some((access, expiry(lasts)));
        Ok(account)
    }

    /// Stops a sign-in that is waiting for the browser.
    pub fn cancel_connect(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Forgets the sign-in on this computer and tells Google to drop it.
    /// Nothing in Drive is touched.
    pub fn disconnect(&self) -> Result<()> {
        // The file first: a sign-in that could not be forgotten here stays
        // as it was, rather than coming back revoked at the next start.
        match std::fs::remove_file(self.data_dir.join(TOKENS_NAME)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(anyhow!("the sign-in could not be forgotten ({e})")),
        }
        let saved = lock(&self.saved).take();
        *lock(&self.access) = None;
        if let Some(saved) = saved {
            // Best effort: the token is forgotten here whatever Google says.
            let _ = tauri::async_runtime::block_on(async {
                http()?
                    .post(REVOKE_URL)
                    .form(&[("token", saved.refresh_token.as_str())])
                    .send()
                    .await
                    .map_err(|e| anyhow!("{}", e.without_url()))
            });
        }
        Ok(())
    }

    /// An access token, fetched anew with the refresh token when the last
    /// one is about to run out. A refused refresh token marks the sign-in
    /// lost and fails with [`NeedsReconnect`].
    pub fn token(&self) -> Result<String> {
        if let Some((token, until)) = lock(&self.access).as_ref() {
            if Instant::now() < *until {
                return Ok(token.clone());
            }
        }
        let client = client().ok_or_else(|| anyhow!("unavailable"))?;
        let saved = lock(&self.saved).clone();
        let Some(saved) = saved.filter(|saved| !saved.lost) else {
            return Err(NeedsReconnect.into());
        };
        let answer = tauri::async_runtime::block_on(async {
            let response = http()?
                .post(TOKEN_URL)
                .form(&[
                    ("client_id", client.id),
                    ("client_secret", client.secret),
                    ("refresh_token", saved.refresh_token.as_str()),
                    ("grant_type", "refresh_token"),
                ])
                .send()
                .await
                .map_err(reached)?;
            let status = response.status().as_u16();
            let body: Value = response.json().await.unwrap_or(Value::Null);
            Ok::<_, anyhow::Error>((status, body))
        })?;
        match answer {
            (200, body) => {
                let token = body["access_token"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Google gave no access token"))?
                    .to_string();
                let lasts = body["expires_in"].as_u64().unwrap_or(3_000);
                {
                    // Disconnected while this was on its way: not kept.
                    let still = lock(&self.saved);
                    if still.is_none() {
                        return Err(NeedsReconnect.into());
                    }
                    *lock(&self.access) = Some((token.clone(), expiry(lasts)));
                }
                if saved.account.is_empty() {
                    if let Ok(account) = account_of(&token) {
                        self.update_saved(|saved| saved.account = account);
                    }
                }
                Ok(token)
            }
            (400 | 401, body) if body["error"] == "invalid_grant" => {
                self.update_saved(|saved| saved.lost = true);
                Err(NeedsReconnect.into())
            }
            (status, _) => Err(HttpStatus(status).into()),
        }
    }

    fn update_saved(&self, change: impl FnOnce(&mut Saved)) {
        let mut guard = lock(&self.saved);
        if let Some(saved) = guard.as_mut() {
            change(saved);
            if let Err(e) = save(&self.data_dir, saved) {
                log::warn!("Could not save the Google sign-in: {e:#}");
            }
        }
    }

    /// Runs a Drive call with an access token; one refused token is
    /// fetched anew and tried again, a second refusal needs the user.
    pub fn call<T>(&self, run: impl Fn(&str) -> Result<T>) -> Result<T> {
        let mut retried = false;
        loop {
            let token = self.token()?;
            match run(&token) {
                Err(e) if refused(&e) => {
                    *lock(&self.access) = None;
                    if retried {
                        self.update_saved(|saved| saved.lost = true);
                        return Err(NeedsReconnect.into());
                    }
                    retried = true;
                }
                other => return other,
            }
        }
    }
}

/// A 401 (the token), or a 403 that says the permission is missing.
fn refused(error: &anyhow::Error) -> bool {
    matches!(error.downcast_ref::<HttpStatus>(), Some(HttpStatus(401)))
}

fn expiry(lasts_secs: u64) -> Instant {
    // A minute early, so a token never runs out in the middle of a call.
    Instant::now() + Duration::from_secs(lasts_secs.saturating_sub(60).max(30))
}

fn load(data_dir: &Path) -> Option<Saved> {
    let text = std::fs::read_to_string(data_dir.join(TOKENS_NAME)).ok()?;
    serde_json::from_str::<Saved>(&text)
        .ok()
        .filter(|saved| !saved.refresh_token.is_empty())
}

/// Writes the sign-in so only the user can read it: on macOS the file is
/// made 0600 before anything is written to it; on Windows it inherits the
/// user's own profile folder's access (the data folder is in `%APPDATA%`).
/// A new file is written beside the old one and renamed over it.
fn save(data_dir: &Path, saved: &Saved) -> Result<()> {
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("could not create {}", data_dir.display()))?;
    let path = data_dir.join(TOKENS_NAME);
    let partial = data_dir.join(format!("{TOKENS_NAME}.partial"));
    let json = serde_json::to_string(saved)?;
    let _ = std::fs::remove_file(&partial);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&partial)
        .context("could not write the sign-in")?;
    file.write_all(json.as_bytes())
        .and_then(|()| file.sync_all())
        .context("could not write the sign-in")?;
    drop(file);
    std::fs::rename(&partial, &path).context("could not save the sign-in")?;
    Ok(())
}

fn http() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent(crate::brand::USER_AGENT)
        .build()?)
}

/// `n` characters a PKCE verifier may hold, from the system's randomness.
fn random_text(n: usize) -> Result<String> {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    // 256 is not a multiple of the 66 characters: bytes past the last whole
    // round are dropped, so no character comes up more often than another.
    let limit = 256 / CHARS.len() * CHARS.len();
    let mut out = String::with_capacity(n);
    while out.len() < n {
        let mut bytes = [0u8; 64];
        getrandom::fill(&mut bytes).map_err(|e| anyhow!("no randomness ({e})"))?;
        for byte in bytes.into_iter().map(usize::from).filter(|&b| b < limit) {
            if out.len() == n {
                break;
            }
            out.push(CHARS[byte % CHARS.len()] as char);
        }
    }
    Ok(out)
}

/// The PKCE S256 challenge of `verifier`.
fn challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn auth_url(client_id: &str, redirect: &str, challenge: &str, state: &str) -> String {
    let mut url = reqwest::Url::parse(AUTH_URL).expect("a fixed address");
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect)
        .append_pair("response_type", "code")
        .append_pair("scope", SCOPE)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent");
    url.to_string()
}

/// What the browser's request to the loopback address said.
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    Code(String),
    Refused(String),
    /// Not the answer (a stray request, the icon, another sign-in's state).
    Other,
}

/// Reads the answer from the first line of the browser's request.
fn read_answer(request_line: &str, state: &str) -> Answer {
    let mut parts = request_line.split_whitespace();
    let (Some("GET"), Some(target)) = (parts.next(), parts.next()) else {
        return Answer::Other;
    };
    let Ok(url) = reqwest::Url::parse(&format!("http://127.0.0.1{target}")) else {
        return Answer::Other;
    };
    if url.path() != "/" {
        return Answer::Other;
    }
    let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    let get = |key: &str| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    if get("state").as_deref() != Some(state) {
        return Answer::Other;
    }
    if let Some(error) = get("error") {
        return Answer::Refused(error);
    }
    match get("code") {
        Some(code) if !code.is_empty() => Answer::Code(code),
        _ => Answer::Other,
    }
}

fn wait_for_code(
    listener: &TcpListener,
    state: &str,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<String> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + timeout;
    loop {
        if cancel.load(Ordering::SeqCst) {
            bail!("cancelled");
        }
        if Instant::now() > deadline {
            bail!("timed_out");
        }
        match listener.accept() {
            Ok((stream, _)) => match answer_browser(stream, state) {
                Answer::Code(code) => return Ok(code),
                Answer::Refused(error) if error == "access_denied" => bail!("denied"),
                Answer::Refused(error) => bail!("Google refused the sign-in ({error})"),
                Answer::Other => {}
            },
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(anyhow!("the browser's answer could not be read ({e})")),
        }
    }
}

/// Reads one request and answers it with a short page.
fn answer_browser(mut stream: TcpStream, state: &str) -> Answer {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut head = Vec::new();
    let mut buf = [0u8; 2048];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < 16_384 {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => head.extend_from_slice(&buf[..n]),
        }
    }
    let text = String::from_utf8_lossy(&head);
    let answer = read_answer(text.lines().next().unwrap_or(""), state);
    let (status, body) = match &answer {
        Answer::Code(_) => ("200 OK", PAGE_DONE),
        Answer::Refused(_) => ("200 OK", PAGE_REFUSED),
        Answer::Other => ("404 Not Found", ""),
    };
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(reply.as_bytes());
    answer
}

const PAGE_DONE: &str = "<!doctype html><meta charset=utf-8><title>서기</title>\
<body style=\"font-family:sans-serif;background:#1b1a17;color:#ece8df;padding:48px\">\
<p>서기에 구글을 연결했습니다.</p><p style=\"opacity:.6\">이 창은 닫아도 됩니다. Seogi is connected; you can close this window.</p>";
const PAGE_REFUSED: &str = "<!doctype html><meta charset=utf-8><title>서기</title>\
<body style=\"font-family:sans-serif;background:#1b1a17;color:#ece8df;padding:48px\">\
<p>연결하지 않았습니다.</p><p style=\"opacity:.6\">서기로 돌아가 다시 해 볼 수 있습니다. Not connected; you can try again from Seogi.</p>";

// ----- Drive -------------------------------------------------------------

/// A file in the phone's folder, as Drive lists it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub mime_type: String,
    /// Drive sends it as a string.
    #[serde(default, deserialize_with = "number_in_text")]
    pub size: Option<u64>,
    /// Drive's own SHA-256 of the content, for files with content.
    #[serde(default)]
    pub sha256_checksum: Option<String>,
    #[serde(default)]
    pub modified_time: String,
    #[serde(default)]
    pub created_time: String,
}

fn number_in_text<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    let text: Option<String> = Option::deserialize(d)?;
    Ok(text.and_then(|t| t.parse().ok()))
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

async fn checked(response: reqwest::Response) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    if status.as_u16() == 403 && body.contains("insufficient") {
        // The permission was taken back: a new sign-in is needed.
        return Err(HttpStatus(401).into());
    }
    Err(HttpStatus(status.as_u16()).into())
}

fn reached(e: reqwest::Error) -> anyhow::Error {
    anyhow::Error::new(Offline(e.without_url().to_string()))
}

/// The search for the marked recordings folder.
fn folder_query() -> String {
    format!(
        "appProperties has {{ key='{MARK_KEY}' and value='{MARK_VALUE}' }} and mimeType = '{FOLDER_MIME}' and trashed = false"
    )
}

/// A value inside single quotes in a Drive search.
fn quoted(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

/// The shared folder: the oldest marked one, made (with the marker, as the
/// phone makes it) when there is none, so a phone signed in later uploads
/// into the same one. One still named [`OLD_FOLDER_NAME`] is renamed.
pub fn find_or_make_folder(token: &str) -> Result<String> {
    tauri::async_runtime::block_on(async {
        let client = http()?;
        let found: Value = checked(
            client
                .get(format!("{API}/files"))
                .header("Authorization", bearer(token))
                .query(&[
                    ("spaces", "drive"),
                    ("orderBy", "createdTime"),
                    ("fields", "files(id,name,createdTime)"),
                    ("q", folder_query().as_str()),
                ])
                .send()
                .await
                .map_err(reached)?,
        )
        .await?
        .json()
        .await
        .map_err(reached)?;
        if let Some(id) = found["files"][0]["id"].as_str() {
            if found["files"][0]["name"].as_str() == Some(OLD_FOLDER_NAME) {
                // Only the name changes; the folder, its id and what is in
                // it stay. A failed rename is tried at the next look.
                let renamed = client
                    .patch(format!("{API}/files/{id}"))
                    .header("Authorization", bearer(token))
                    .query(&[("fields", "id")])
                    .json(&serde_json::json!({ "name": FOLDER_NAME }))
                    .send()
                    .await
                    .map_err(reached);
                match renamed {
                    Ok(response) => {
                        if let Err(e) = checked(response).await {
                            log::warn!("The Drive folder could not be renamed yet: {e:#}");
                        }
                    }
                    Err(e) => log::warn!("The Drive folder could not be renamed yet: {e:#}"),
                }
            }
            return Ok(id.to_string());
        }
        let made: Value = checked(
            client
                .post(format!("{API}/files"))
                .header("Authorization", bearer(token))
                .query(&[("fields", "id")])
                .json(&serde_json::json!({
                    "name": FOLDER_NAME,
                    "mimeType": FOLDER_MIME,
                    "appProperties": { MARK_KEY: MARK_VALUE },
                }))
                .send()
                .await
                .map_err(reached)?,
        )
        .await?
        .json()
        .await
        .map_err(reached)?;
        made["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("Drive made no folder"))
    })
}

/// Every file in `folder` that is not in the trash.
pub fn list_folder(token: &str, folder: &str) -> Result<Vec<DriveFile>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Page {
        #[serde(default)]
        files: Vec<DriveFile>,
        next_page_token: Option<String>,
    }
    let q = format!("'{}' in parents and trashed = false", quoted(folder));
    tauri::async_runtime::block_on(async {
        let client = http()?;
        let mut all = Vec::new();
        let mut next: Option<String> = None;
        loop {
            let mut query = vec![
                ("spaces", "drive".to_string()),
                ("pageSize", "200".to_string()),
                ("orderBy", "createdTime".to_string()),
                (
                    "fields",
                    "nextPageToken,files(id,name,mimeType,size,sha256Checksum,modifiedTime,createdTime)"
                        .to_string(),
                ),
                ("q", q.clone()),
            ];
            if let Some(token) = &next {
                query.push(("pageToken", token.clone()));
            }
            let page: Page = checked(
                client
                    .get(format!("{API}/files"))
                    .header("Authorization", bearer(token))
                    .query(&query)
                    .send()
                    .await
                    .map_err(reached)?,
            )
            .await?
            .json()
            .await
            .map_err(reached)?;
            all.extend(page.files);
            match page.next_page_token {
                Some(token) if !token.is_empty() => next = Some(token),
                _ => return Ok(all),
            }
        }
    })
}

/// The content of a small file (a sidecar), at most `limit` bytes.
pub fn read_small(token: &str, id: &str, limit: usize) -> Result<Vec<u8>> {
    tauri::async_runtime::block_on(async {
        let mut response = checked(
            http()?
                .get(format!("{API}/files/{id}"))
                .header("Authorization", bearer(token))
                .query(&[("alt", "media")])
                .send()
                .await
                .map_err(reached)?,
        )
        .await?;
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(reached)? {
            body.extend_from_slice(&chunk);
            if body.len() > limit {
                bail!("the file is larger than a sidecar can be");
            }
        }
        Ok(body)
    })
}

/// Downloads a file's content to `to` (made anew). Returns its size.
pub fn download(token: &str, id: &str, to: &Path) -> Result<u64> {
    tauri::async_runtime::block_on(async {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(120))
            .user_agent(crate::brand::USER_AGENT)
            .build()?;
        let mut response = checked(
            client
                .get(format!("{API}/files/{id}"))
                .header("Authorization", bearer(token))
                .query(&[("alt", "media")])
                .send()
                .await
                .map_err(reached)?,
        )
        .await?;
        let mut file = std::fs::File::create(to)
            .with_context(|| format!("could not create {}", to.display()))?;
        let mut written = 0u64;
        while let Some(chunk) = response.chunk().await.map_err(reached)? {
            file.write_all(&chunk)
                .with_context(|| format!("could not write {}", to.display()))?;
            written += chunk.len() as u64;
        }
        file.sync_all().ok();
        Ok(written)
    })
}

/// What Drive says about one file or folder, for moving and renaming.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub parents: Vec<String>,
    #[serde(default)]
    pub trashed: bool,
    #[serde(default)]
    pub sha256_checksum: Option<String>,
}

/// One file or folder; `None` when Drive no longer has it (or this app may
/// no longer see it).
pub fn file_info(token: &str, id: &str) -> Result<Option<FileInfo>> {
    tauri::async_runtime::block_on(async {
        let response = http()?
            .get(format!("{API}/files/{id}"))
            .header("Authorization", bearer(token))
            .query(&[("fields", "id,name,parents,trashed,sha256Checksum")])
            .send()
            .await
            .map_err(reached)?;
        if response.status().as_u16() == 404 {
            return Ok(None);
        }
        Ok(Some(
            checked(response).await?.json().await.map_err(reached)?,
        ))
    })
}

/// The first file in `parent` (not in the trash) matching `q`, a Drive
/// search added to the parent's.
async fn find_in(token: &str, parent: &str, q: &str) -> Result<Option<FileInfo>> {
    let query = format!(
        "'{}' in parents and trashed = false and {q}",
        quoted(parent)
    );
    let found: Value = checked(
        http()?
            .get(format!("{API}/files"))
            .header("Authorization", bearer(token))
            .query(&[
                ("spaces", "drive"),
                ("orderBy", "createdTime"),
                ("fields", "files(id,name,parents,trashed,sha256Checksum)"),
                ("q", query.as_str()),
            ])
            .send()
            .await
            .map_err(reached)?,
    )
    .await?
    .json()
    .await
    .map_err(reached)?;
    Ok(found["files"]
        .get(0)
        .and_then(|file| serde_json::from_value(file.clone()).ok()))
}

/// The folder in `parent` marked as the meeting `meeting_id`'s.
pub fn find_meeting_folder(token: &str, parent: &str, meeting_id: &str) -> Result<Option<String>> {
    let q = format!(
        "mimeType = '{FOLDER_MIME}' and appProperties has {{ key='{MEETING_KEY}' and value='{}' }}",
        quoted(meeting_id)
    );
    tauri::async_runtime::block_on(async {
        Ok(find_in(token, parent, &q).await?.map(|file| file.id))
    })
}

/// The file named `name` in `parent`, if there is one.
pub fn find_named(token: &str, parent: &str, name: &str) -> Result<Option<FileInfo>> {
    let q = format!("name = '{}'", quoted(name));
    tauri::async_runtime::block_on(find_in(token, parent, &q))
}

/// Makes a meeting's folder in `parent`, marked with its id.
pub fn make_meeting_folder(
    token: &str,
    parent: &str,
    name: &str,
    meeting_id: &str,
) -> Result<String> {
    tauri::async_runtime::block_on(async {
        let made: Value = checked(
            http()?
                .post(format!("{API}/files"))
                .header("Authorization", bearer(token))
                .query(&[("fields", "id")])
                .json(&serde_json::json!({
                    "name": name,
                    "mimeType": FOLDER_MIME,
                    "parents": [parent],
                    "appProperties": { MEETING_KEY: meeting_id },
                }))
                .send()
                .await
                .map_err(reached)?,
        )
        .await?
        .json()
        .await
        .map_err(reached)?;
        made["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("Drive made no folder"))
    })
}

/// Changes a file's metadata (`fields` as Drive takes them) and, with
/// `add`, moves it: into `add`, out of `remove`.
fn patch(token: &str, id: &str, body: Value, add: Option<&str>, remove: &[String]) -> Result<()> {
    tauri::async_runtime::block_on(async {
        let mut query: Vec<(&str, String)> = vec![("fields", "id".to_string())];
        if let Some(add) = add {
            query.push(("addParents", add.to_string()));
            if !remove.is_empty() {
                query.push(("removeParents", remove.join(",")));
            }
        }
        checked(
            http()?
                .patch(format!("{API}/files/{id}"))
                .header("Authorization", bearer(token))
                .query(&query)
                .json(&body)
                .send()
                .await
                .map_err(reached)?,
        )
        .await?;
        Ok(())
    })
}

pub fn rename(token: &str, id: &str, name: &str) -> Result<()> {
    patch(token, id, serde_json::json!({ "name": name }), None, &[])
}

/// Moves a file into `to`, out of the folders it was in (`from`). The
/// file itself, its id and its content stay as they are.
pub fn move_into(token: &str, id: &str, to: &str, from: &[String]) -> Result<()> {
    patch(token, id, serde_json::json!({}), Some(to), from)
}

/// Puts a file or folder in Drive's trash, where the user can take it back
/// for 30 days. Nothing is deleted for good.
pub fn trash(token: &str, id: &str) -> Result<()> {
    patch(token, id, serde_json::json!({ "trashed": true }), None, &[])
}

/// A small file made in `parent` in one request.
pub fn create_small(
    token: &str,
    parent: &str,
    name: &str,
    mime: &str,
    bytes: &[u8],
) -> Result<String> {
    let boundary = format!("seogi-{}", random_text(24)?.replace(['.', '~'], "-"));
    let metadata = serde_json::json!({ "name": name, "parents": [parent] });
    let mut body = Vec::with_capacity(bytes.len() + 512);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{metadata}\r\n--{boundary}\r\nContent-Type: {mime}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    tauri::async_runtime::block_on(async {
        let made: Value = checked(
            http()?
                .post(format!("{UPLOAD_API}/files"))
                .header("Authorization", bearer(token))
                .header(
                    "Content-Type",
                    format!("multipart/related; boundary={boundary}"),
                )
                .query(&[("uploadType", "multipart"), ("fields", "id")])
                .body(body)
                .send()
                .await
                .map_err(reached)?,
        )
        .await?
        .json()
        .await
        .map_err(reached)?;
        made["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("Drive made no file"))
    })
}

/// Gives a small file this app made new content.
pub fn update_small(token: &str, id: &str, mime: &str, bytes: &[u8]) -> Result<()> {
    let bytes = bytes.to_vec();
    tauri::async_runtime::block_on(async {
        checked(
            http()?
                .patch(format!("{UPLOAD_API}/files/{id}"))
                .header("Authorization", bearer(token))
                .header("Content-Type", mime)
                .query(&[("uploadType", "media"), ("fields", "id")])
                .body(bytes)
                .send()
                .await
                .map_err(reached)?,
        )
        .await?;
        Ok(())
    })
}

/// How many bytes the account has left; `None` when it has no limit.
pub fn free_space(token: &str) -> Result<Option<u64>> {
    tauri::async_runtime::block_on(async {
        let about: Value = checked(
            http()?
                .get(format!("{API}/about"))
                .header("Authorization", bearer(token))
                .query(&[("fields", "storageQuota(limit,usage)")])
                .send()
                .await
                .map_err(reached)?,
        )
        .await?
        .json()
        .await
        .map_err(reached)?;
        let number = |key: &str| {
            about["storageQuota"][key]
                .as_str()
                .and_then(|text| text.parse::<u64>().ok())
        };
        Ok(number("limit").map(|limit| limit.saturating_sub(number("usage").unwrap_or(0))))
    })
}

/// Where a resumable upload stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Upload {
    /// Drive has the first this many bytes.
    At(u64),
    /// Drive has it all: the new file's id.
    Done(String),
    /// The upload's address no longer works (a week old, or refused): it
    /// starts again from the beginning.
    Expired,
}

/// Starts a resumable upload of `size` bytes into `parent`; returns the
/// address the bytes go to. The address is kept, so an upload cut off
/// (the computer asleep, the app quit) goes on where it stopped.
pub fn start_upload(
    token: &str,
    parent: &str,
    name: &str,
    mime: &str,
    size: u64,
) -> Result<String> {
    tauri::async_runtime::block_on(async {
        let response = checked(
            http()?
                .post(format!("{UPLOAD_API}/files"))
                .header("Authorization", bearer(token))
                .header("X-Upload-Content-Type", mime)
                .header("X-Upload-Content-Length", size.to_string())
                .query(&[("uploadType", "resumable"), ("fields", "id")])
                .json(&serde_json::json!({ "name": name, "parents": [parent] }))
                .send()
                .await
                .map_err(reached)?,
        )
        .await?;
        response
            .headers()
            .get("location")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
            .ok_or_else(|| anyhow!("Drive gave no address for the upload"))
    })
}

/// What Drive's answer to an upload request says.
async fn upload_answer(response: reqwest::Response) -> Result<Upload> {
    match response.status().as_u16() {
        200 | 201 => {
            let made: Value = response.json().await.map_err(reached)?;
            made["id"]
                .as_str()
                .map(|id| Upload::Done(id.to_string()))
                .ok_or_else(|| anyhow!("Drive made no file"))
        }
        308 => Ok(Upload::At(
            response
                .headers()
                .get("range")
                .and_then(|value| value.to_str().ok())
                .and_then(range_end)
                .map_or(0, |end| end + 1),
        )),
        404 | 410 => Ok(Upload::Expired),
        _ => Err(checked(response)
            .await
            .err()
            .unwrap_or_else(|| anyhow!("unexpected"))),
    }
}

/// The last byte in a `Range: bytes=0-1234` answer.
fn range_end(range: &str) -> Option<u64> {
    range
        .trim()
        .strip_prefix("bytes=")?
        .split('-')
        .nth(1)?
        .trim()
        .parse()
        .ok()
}

/// Asks how much of an upload Drive has.
pub fn upload_offset(token: &str, session: &str, size: u64) -> Result<Upload> {
    tauri::async_runtime::block_on(async {
        let response = http()?
            .put(session)
            .header("Authorization", bearer(token))
            .header("Content-Range", format!("bytes */{size}"))
            .header("Content-Length", "0")
            .send()
            .await
            .map_err(reached)?;
        upload_answer(response).await
    })
}

/// The bytes of an upload sent in one request: a multiple of 256 KiB, as
/// Drive asks.
pub const UPLOAD_CHUNK: u64 = 8 * 1024 * 1024;

/// Sends the next part of `file` (of `size` bytes) from `offset`.
pub fn upload_chunk(
    token: &str,
    session: &str,
    file: &Path,
    offset: u64,
    size: u64,
) -> Result<Upload> {
    use std::io::{Seek, SeekFrom};
    let len = UPLOAD_CHUNK.min(size.saturating_sub(offset));
    let mut bytes = vec![0u8; len as usize];
    let mut opened =
        std::fs::File::open(file).with_context(|| format!("could not read {}", file.display()))?;
    opened.seek(SeekFrom::Start(offset))?;
    opened
        .read_exact(&mut bytes)
        .with_context(|| format!("could not read {}", file.display()))?;
    tauri::async_runtime::block_on(async {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(10 * 60))
            .user_agent(crate::brand::USER_AGENT)
            .build()?;
        let range = if len == 0 {
            format!("bytes */{size}")
        } else {
            format!("bytes {offset}-{}/{size}", offset + len - 1)
        };
        let response = client
            .put(session)
            .header("Authorization", bearer(token))
            .header("Content-Range", range)
            .body(bytes)
            .send()
            .await
            .map_err(reached)?;
        upload_answer(response).await
    })
}

/// The signed-in account's address.
fn account_of(token: &str) -> Result<String> {
    tauri::async_runtime::block_on(async {
        let about: Value = checked(
            http()?
                .get(format!("{API}/about"))
                .header("Authorization", bearer(token))
                .query(&[("fields", "user(emailAddress)")])
                .send()
                .await
                .map_err(reached)?,
        )
        .await?
        .json()
        .await
        .map_err(reached)?;
        about["user"]["emailAddress"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("Drive did not say whose account it is"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_challenge_is_rfc_7636s() {
        // The example in RFC 7636, appendix B.
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn random_text_is_long_enough_and_allowed() {
        let a = random_text(64).unwrap();
        let b = random_text(64).unwrap();
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert!(a
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._~".contains(c)));
    }

    #[test]
    fn the_sign_in_asks_for_drive_file_only_with_pkce() {
        let url =
            reqwest::Url::parse(&auth_url("id", "http://127.0.0.1:5000", "ch", "st")).unwrap();
        let pairs: std::collections::HashMap<String, String> =
            url.query_pairs().into_owned().collect();
        assert_eq!(pairs["scope"], SCOPE);
        assert_eq!(pairs["redirect_uri"], "http://127.0.0.1:5000");
        assert_eq!(pairs["code_challenge"], "ch");
        assert_eq!(pairs["code_challenge_method"], "S256");
        assert_eq!(pairs["state"], "st");
        assert_eq!(pairs["access_type"], "offline");
    }

    #[test]
    fn only_the_answer_with_this_state_counts() {
        assert_eq!(
            read_answer("GET /?state=st&code=4%2Fabc&scope=x HTTP/1.1", "st"),
            Answer::Code("4/abc".into())
        );
        assert_eq!(
            read_answer("GET /?state=st&error=access_denied HTTP/1.1", "st"),
            Answer::Refused("access_denied".into())
        );
        assert_eq!(
            read_answer("GET /?state=other&code=x HTTP/1.1", "st"),
            Answer::Other
        );
        assert_eq!(
            read_answer("GET /favicon.ico HTTP/1.1", "st"),
            Answer::Other
        );
        assert_eq!(
            read_answer("POST /?state=st&code=x HTTP/1.1", "st"),
            Answer::Other
        );
        assert_eq!(read_answer("", "st"), Answer::Other);
    }

    #[test]
    fn the_browser_is_answered_and_the_code_taken() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let browser = std::thread::spawn(move || {
            // The icon first, then the answer.
            let mut icon = TcpStream::connect(("127.0.0.1", port)).unwrap();
            icon.write_all(b"GET /favicon.ico HTTP/1.1\r\nHost: x\r\n\r\n")
                .unwrap();
            let mut reply = String::new();
            icon.read_to_string(&mut reply).unwrap();
            assert!(reply.starts_with("HTTP/1.1 404"));
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            stream
                .write_all(b"GET /?state=st&code=abc HTTP/1.1\r\nHost: x\r\n\r\n")
                .unwrap();
            let mut reply = String::new();
            stream.read_to_string(&mut reply).unwrap();
            reply
        });
        let cancel = AtomicBool::new(false);
        let code = wait_for_code(&listener, "st", &cancel, Duration::from_secs(10)).unwrap();
        assert_eq!(code, "abc");
        let reply = browser.join().unwrap();
        assert!(reply.starts_with("HTTP/1.1 200"));
        assert!(reply.contains("연결했습니다"));
    }

    #[test]
    fn waiting_stops_when_cancelled() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let cancel = AtomicBool::new(true);
        let error = wait_for_code(&listener, "st", &cancel, Duration::from_secs(10)).unwrap_err();
        assert_eq!(error.to_string(), "cancelled");
    }

    #[test]
    fn the_sign_in_is_kept_private_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let google = Google::new(dir.path());
        assert_eq!(google.standing(), Standing::NotConnected);
        let saved = Saved {
            refresh_token: "r".into(),
            account: "someone@example.com".into(),
            lost: false,
        };
        save(dir.path(), &saved).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join(TOKENS_NAME))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let google = Google::new(dir.path());
        assert_eq!(
            google.standing(),
            Standing::Connected {
                account: "someone@example.com".into()
            }
        );
        google.update_saved(|saved| saved.lost = true);
        assert_eq!(
            Google::new(dir.path()).standing(),
            Standing::Lost {
                account: "someone@example.com".into()
            }
        );
        // A lost sign-in is not tried again until the user connects.
        if available() {
            assert!(google
                .token()
                .unwrap_err()
                .downcast_ref::<NeedsReconnect>()
                .is_some());
        }
    }

    #[test]
    fn drive_lists_sizes_as_text_and_searches_by_the_marker() {
        let file: DriveFile = serde_json::from_str(
            r#"{"id":"A","name":"a.m4a","mimeType":"audio/mp4","size":"1234","sha256Checksum":"ab"}"#,
        )
        .unwrap();
        assert_eq!(file.size, Some(1234));
        assert_eq!(file.sha256_checksum.as_deref(), Some("ab"));
        assert!(folder_query()
            .contains("appProperties has { key='seogi' and value='recordings-root' }"));
        assert!(folder_query().contains("trashed = false"));
        assert_eq!(quoted(r"a'b\c"), r"a\'b\\c");
    }

    #[test]
    fn an_upload_goes_on_after_the_last_byte_drive_has() {
        assert_eq!(range_end("bytes=0-1048575"), Some(1_048_575));
        assert_eq!(range_end(" bytes=0-0 "), Some(0));
        assert_eq!(range_end("nonsense"), None);
        assert_eq!(UPLOAD_CHUNK % (256 * 1024), 0);
    }
}
