//! The language model that writes minutes: one request in, one JSON object
//! out, whichever provider the user chose.
//!
//! Every provider is asked for JSON that fits a schema. The ones that can
//! enforce a schema do; the answer is parsed leniently either way, because a
//! local model sometimes wraps its JSON in a code fence or a sentence.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use log::{debug, warn};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use specta::Type;

use super::platform::{find_program, hide_console};

/// How long one request may take before it is given up on.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Where the summary model runs. The default is local and free.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Provider {
    /// Ollama on this computer.
    Ollama { base_url: String, model: String },
    /// Claude Code's command-line tool, signed in with the user's own plan.
    /// An empty model leaves the choice to the tool.
    ClaudeCli { model: String },
    /// OpenAI's Codex command-line tool, signed in with the user's own plan.
    /// An empty model leaves the choice to the tool.
    CodexCli { model: String },
    /// An OpenAI-compatible endpoint with the user's own key: OpenAI, Gemini,
    /// Groq, OpenRouter, LM Studio and others. `key` names the key in the
    /// operating system's keychain; empty when the endpoint needs none.
    OpenAiCompatible {
        base_url: String,
        model: String,
        key: String,
    },
    /// Anthropic's API with the user's own key.
    Anthropic { model: String, key: String },
}

impl Provider {
    pub fn default_ollama() -> Self {
        Provider::Ollama {
            base_url: "http://localhost:11434".to_string(),
            model: "gemma3:12b".to_string(),
        }
    }

    /// A provider from a short spec, as the command line takes it:
    /// `ollama:gemma3:12b`, `claude:sonnet`, `codex`, `codex:gpt-5`.
    pub fn parse(spec: &str) -> Option<Self> {
        let (kind, model) = spec.split_once(':').unwrap_or((spec, ""));
        match kind {
            "ollama" if !model.is_empty() => Some(Provider::Ollama {
                base_url: "http://localhost:11434".to_string(),
                model: model.to_string(),
            }),
            "claude" => Some(Provider::ClaudeCli {
                model: if model.is_empty() { "sonnet" } else { model }.to_string(),
            }),
            "codex" => Some(Provider::CodexCli {
                model: model.to_string(),
            }),
            _ => None,
        }
    }

    /// A short name for logs and the minutes footer: `ollama gemma3:12b`.
    pub fn label(&self) -> String {
        match self {
            Provider::Ollama { model, .. } => format!("ollama {model}"),
            Provider::ClaudeCli { model } if model.is_empty() => "claude".to_string(),
            Provider::ClaudeCli { model } => format!("claude {model}"),
            Provider::CodexCli { model } if model.is_empty() => "codex".to_string(),
            Provider::CodexCli { model } => format!("codex {model}"),
            Provider::OpenAiCompatible { model, .. } => model.clone(),
            Provider::Anthropic { model, .. } => model.clone(),
        }
    }

    /// How many requests may be in flight at once. A model on this computer
    /// answers one at a time whatever is asked of it, and an API key has a
    /// rate limit that a burst runs into. A signed-in command-line tool takes
    /// a few side by side, which is what keeps a meeting of two hours from
    /// taking an hour to write up.
    pub fn at_once(&self) -> usize {
        match self {
            Provider::ClaudeCli { .. } | Provider::CodexCli { .. } => 3,
            _ => 1,
        }
    }

    /// The same tool with the model it picks itself, for when the model the
    /// user chose is refused. `None` when there is nothing to fall back to:
    /// the tool's own choice is already in use, or the provider has no such
    /// default.
    pub fn automatic(&self) -> Option<Self> {
        match self {
            Provider::ClaudeCli { model } if !model.is_empty() => Some(Provider::ClaudeCli {
                model: String::new(),
            }),
            Provider::CodexCli { model } if !model.is_empty() => Some(Provider::CodexCli {
                model: String::new(),
            }),
            _ => None,
        }
    }

    /// The provider with a model from its tool's list (`codex` is Codex's,
    /// as [`codex_models`] gives it). A name typed in earlier becomes the
    /// listed model it names ("Astra" is GPT-6.2 Astra), and anything else
    /// becomes the recommended one, so a saved setting can never be a name
    /// the tool refuses.
    pub fn tidied(&self, codex: &[ModelChoice]) -> Self {
        match self {
            Provider::ClaudeCli { model } => Provider::ClaudeCli {
                model: listed(model, &claude_models()),
            },
            Provider::CodexCli { model } => Provider::CodexCli {
                model: listed(model, codex),
            },
            _ => self.clone(),
        }
    }

    /// Whether the text leaves this computer.
    pub fn is_local(&self) -> bool {
        match self {
            Provider::Ollama { base_url, .. } | Provider::OpenAiCompatible { base_url, .. } => {
                base_url.contains("localhost") || base_url.contains("127.0.0.1")
            }
            _ => false,
        }
    }
}

/// One request: instructions, the material, and the shape of the answer.
pub struct Request<'a> {
    pub system: &'a str,
    pub user: &'a str,
    /// JSON Schema of the object to return.
    pub schema: &'a Value,
}

/// Looks up a secret by name. The pipeline passes the keychain; tests pass a
/// closure.
pub type Secrets<'a> = &'a (dyn Fn(&str) -> Option<String> + Sync);

/// Providers whose chosen model was refused while this app has been
/// running. Their requests go to the tool's own choice of model instead.
static REFUSED: std::sync::Mutex<Vec<Provider>> = std::sync::Mutex::new(Vec::new());

/// Whether `provider`'s model was refused, so its tool's own choice is used.
pub fn model_refused(provider: &Provider) -> bool {
    REFUSED
        .lock()
        .map(|refused| refused.contains(provider))
        .unwrap_or(false)
}

/// The provider requests actually go to: the one chosen, or its tool's own
/// choice of model once the chosen model has been refused.
pub fn in_use(provider: &Provider) -> Provider {
    match provider.automatic() {
        Some(automatic) if model_refused(provider) => automatic,
        _ => provider.clone(),
    }
}

/// Whether an error says the model itself cannot be used: a name the tool
/// does not know, or one the user's plan does not include.
pub fn refuses_model(error: &str) -> bool {
    let error = error.to_lowercase();
    error.contains("model")
        && [
            "not supported",
            "not found",
            "does not exist",
            "issue with the selected model",
            "unknown model",
            "unsupported model",
            "invalid model",
            "unrecognized_model",
            "model_not_found",
        ]
        .iter()
        .any(|sign| error.contains(sign))
}

/// Whether an error says the user's plan has run out for now.
pub fn hit_limit(error: &str) -> bool {
    let error = error.to_lowercase();
    [
        "hit your",
        "usage limit",
        "rate limit",
        "quota",
        "429 too many",
    ]
    .iter()
    .any(|sign| error.contains(sign))
}

/// The code a failed minutes run is shown with: why, in the user's words.
pub fn trouble_code(error: &str) -> &'static str {
    if refuses_model(error) {
        "summary_model_refused"
    } else if hit_limit(error) {
        "summary_limit"
    } else {
        "summary_model"
    }
}

/// Sends `request` to `provider` and returns the JSON object it answered
/// with. Blocks; call it from a worker thread.
///
/// A model the tool refuses does not fail the request: it goes to the
/// tool's own choice of model, and so do the requests after it.
pub fn complete(provider: &Provider, request: &Request<'_>, secrets: Secrets<'_>) -> Result<Value> {
    let chosen = in_use(provider);
    match complete_with(&chosen, request, secrets) {
        Err(e) if refuses_model(&format!("{e:#}")) => match chosen.automatic() {
            Some(automatic) => {
                warn!(
                    "{} refused its model ({e:#}); using {} instead",
                    chosen.label(),
                    automatic.label()
                );
                if let Ok(mut refused) = REFUSED.lock() {
                    if !refused.contains(provider) {
                        refused.push(provider.clone());
                    }
                }
                complete_with(&automatic, request, secrets)
            }
            None => Err(e),
        },
        other => other,
    }
}

/// What the error of [`complete_with_images`] says when the provider cannot
/// be shown pictures.
pub const NO_IMAGES: &str = "this way of writing minutes cannot read pictures";

/// Like [`complete`], with pictures shown along with the request. Only the
/// signed-in command-line tools take pictures here; a model on this computer
/// is not given them (it reads them poorly and slowly), nor is a paid key
/// used for them.
pub fn complete_with_images(
    provider: &Provider,
    request: &Request<'_>,
    images: &[PathBuf],
) -> Result<Value> {
    let chosen = in_use(provider);
    let text = match &chosen {
        Provider::ClaudeCli { model } => claude_cli(model, request, images)?,
        Provider::CodexCli { model } => codex_cli(model, request, images)?,
        _ => bail!("{NO_IMAGES}"),
    };
    parse_json(&text).with_context(|| format!("{} did not answer with JSON", chosen.label()))
}

fn complete_with(
    provider: &Provider,
    request: &Request<'_>,
    secrets: Secrets<'_>,
) -> Result<Value> {
    let started = Instant::now();
    let text = match provider {
        Provider::Ollama { base_url, model } => ollama(base_url, model, request)?,
        Provider::ClaudeCli { model } => claude_cli(model, request, &[])?,
        Provider::CodexCli { model } => codex_cli(model, request, &[])?,
        Provider::OpenAiCompatible {
            base_url,
            model,
            key,
        } => {
            let api_key = if key.is_empty() {
                String::new()
            } else {
                secrets(key).ok_or_else(|| anyhow!("no API key is saved for {key}"))?
            };
            open_ai_compatible(base_url, model, api_key, request)?
        }
        Provider::Anthropic { model, key } => {
            let api_key = secrets(key).ok_or_else(|| anyhow!("no API key is saved for {key}"))?;
            anthropic(model, api_key, request)?
        }
    };
    debug!(
        "{} answered {} chars in {:.1}s",
        provider.label(),
        text.len(),
        started.elapsed().as_secs_f64()
    );
    parse_json(&text).with_context(|| format!("{} did not answer with JSON", provider.label()))
}

/// The JSON object in `text`, which may be wrapped in a code fence, a
/// reasoning block or a sentence.
pub fn parse_json(text: &str) -> Result<Value> {
    let mut text = text.trim();
    // A reasoning model may print its thinking first.
    if let Some(end) = text.rfind("</think>") {
        text = text[end + "</think>".len()..].trim();
    }
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        if value.is_object() {
            return Ok(value);
        }
    }
    let start = text
        .find('{')
        .ok_or_else(|| anyhow!("no JSON object in the answer"))?;
    let end = text
        .rfind('}')
        .ok_or_else(|| anyhow!("no JSON object in the answer"))?;
    if end <= start {
        bail!("no JSON object in the answer");
    }
    serde_json::from_str::<Value>(&text[start..=end])
        .context("the answer's JSON could not be read")
        .and_then(|value| {
            if value.is_object() {
                Ok(value)
            } else {
                Err(anyhow!("the answer is not a JSON object"))
            }
        })
}

/// What a provider's error says when the model ran out of room for its
/// answer. Callers look for it to ask about less at a time.
pub const CUT_OFF: &str = "the answer was cut off at the model's output limit";

/// The most tokens a local model may answer with. An answer that needs more
/// is reported as cut off, and the caller asks about less at a time.
const ANSWER_TOKENS: u32 = 8_192;

/// A context size that holds the request and its answer. Korean runs about a
/// token per character, sometimes more.
fn context_tokens(request: &Request<'_>) -> u32 {
    let chars = request.system.chars().count() + request.user.chars().count();
    let needed = chars * 13 / 10 + ANSWER_TOKENS as usize + 500;
    [8_192u32, 16_384, 32_768]
        .into_iter()
        .find(|&size| size as usize >= needed)
        .unwrap_or(32_768)
}

fn ollama(base_url: &str, model: &str, request: &Request<'_>) -> Result<String> {
    let url = format!("{}/api/chat", base_url.trim_end_matches('/'));
    let body = json!({
        "model": model,
        "stream": false,
        "format": request.schema,
        "messages": [
            { "role": "system", "content": request.system },
            { "role": "user", "content": request.user },
        ],
        "options": {
            "temperature": 0.1,
            "num_ctx": context_tokens(request),
            "num_predict": ANSWER_TOKENS,
        },
    });
    let response: Value = tauri::async_runtime::block_on(async {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(REQUEST_TIMEOUT)
            .build()?;
        let response = client.post(&url).json(&body).send().await.map_err(|e| {
            if e.is_connect() {
                anyhow!("Ollama is not running at {base_url}")
            } else {
                anyhow!("the request to Ollama failed: {e}")
            }
        })?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            let reason = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
                .unwrap_or(text);
            bail!("Ollama answered {status}: {reason}");
        }
        serde_json::from_str::<Value>(&text).context("Ollama's answer could not be read")
    })?;
    if response.get("done_reason").and_then(Value::as_str) == Some("length") {
        bail!("{CUT_OFF}");
    }
    response
        .pointer("/message/content")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("Ollama's answer has no text"))
}

fn open_ai_compatible(
    base_url: &str,
    model: &str,
    api_key: String,
    request: &Request<'_>,
) -> Result<String> {
    use crate::settings::{PostProcessApiFormat, PostProcessProvider};
    let provider = PostProcessProvider {
        id: "custom".to_string(),
        label: "custom".to_string(),
        base_url: base_url.to_string(),
        allow_base_url_edit: true,
        models_endpoint: None,
        supports_structured_output: true,
        api_format: PostProcessApiFormat::OpenAi,
    };
    let system = format!(
        "{}\n\nAnswer with one JSON object that fits this JSON Schema:\n{}",
        request.system, request.schema
    );
    let answer = tauri::async_runtime::block_on(async {
        // Schema enforcement is strict on some endpoints and absent on
        // others; the schema in the prompt works on all of them.
        crate::llm_client::send_chat_completion_with_schema(
            &provider,
            api_key,
            model,
            request.user.to_string(),
            Some(system),
            None,
            true,
        )
        .await
    })
    .map_err(|e| anyhow!(e))?;
    answer.ok_or_else(|| anyhow!("the endpoint's answer has no text"))
}

fn anthropic(model: &str, api_key: String, request: &Request<'_>) -> Result<String> {
    use crate::settings::{PostProcessApiFormat, PostProcessProvider};
    let provider = PostProcessProvider {
        id: "anthropic".to_string(),
        label: "Anthropic".to_string(),
        base_url: "https://api.anthropic.com/v1".to_string(),
        allow_base_url_edit: false,
        models_endpoint: None,
        supports_structured_output: false,
        api_format: PostProcessApiFormat::Anthropic,
    };
    let prompt = format!(
        "{}\n\nAnswer with one JSON object that fits this JSON Schema, and nothing else:\n{}\n\n{}",
        request.system, request.schema, request.user
    );
    let answer = tauri::async_runtime::block_on(async {
        crate::llm_client::send_chat_completion(&provider, api_key, model, prompt, true).await
    })
    .map_err(|e| anyhow!(e))?;
    answer.ok_or_else(|| anyhow!("Anthropic's answer has no text"))
}

/// Runs a command-line tool with `input` on its stdin and returns its
/// stdout. The tool is killed if it runs past the request timeout.
fn run_tool(mut command: Command, input: &str, name: &str) -> Result<String> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut command);
    let mut child = command
        .spawn()
        .with_context(|| format!("could not start {name}"))?;
    let mut stdin = child.stdin.take().expect("stdin was piped");
    let input = input.to_string();
    // Written from its own thread: a tool that prints before it has read
    // everything would otherwise block both sides.
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    let read = |mut pipe: Box<dyn std::io::Read + Send>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = pipe.read_to_string(&mut text);
            text
        })
    };
    let out = read(Box::new(stdout));
    let err = read(Box::new(stderr));

    let started = Instant::now();
    let status = loop {
        match child
            .try_wait()
            .with_context(|| format!("could not wait for {name}"))?
        {
            Some(status) => break status,
            None if started.elapsed() > REQUEST_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                bail!(
                    "{name} did not answer within {} minutes",
                    REQUEST_TIMEOUT.as_secs() / 60
                );
            }
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    };
    let _ = writer.join();
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    if !status.success() && stdout.trim().is_empty() {
        let reason = stderr
            .lines()
            .last()
            .unwrap_or("it failed")
            .trim()
            .to_string();
        bail!("{name} failed: {reason}");
    }
    Ok(stdout)
}

/// Where Claude Code's command-line tool is, if it is installed.
pub fn find_claude_cli() -> Option<PathBuf> {
    let home = dirs_home();
    let mut extra = Vec::new();
    if let Some(home) = &home {
        extra.push(home.join(".local").join("bin"));
        extra.push(home.join(".claude").join("local"));
        extra.push(home.join(".claude").join("bin"));
    }
    find_program("claude", &extra)
}

/// Where OpenAI's Codex command-line tool is, if it is installed.
pub fn find_codex_cli() -> Option<PathBuf> {
    let mut extra = Vec::new();
    if let Some(home) = dirs_home() {
        extra.push(home.join(".local").join("bin"));
        extra.push(home.join(".codex").join("bin"));
    }
    find_program("codex", &extra)
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// The media type Claude takes a picture file as, by its name.
fn image_type(path: &std::path::Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

fn claude_cli(model: &str, request: &Request<'_>, images: &[PathBuf]) -> Result<String> {
    let claude = find_claude_cli().ok_or_else(|| anyhow!("Claude Code is not installed"))?;
    let mut command = Command::new(claude);
    command.arg("-p");
    if !model.is_empty() {
        command.args(["--model", model]);
    }
    command
        // A plain question and answer: no tools, no project or user
        // settings, nothing saved.
        .args([
            "--tools",
            "",
            "--strict-mcp-config",
            "--no-session-persistence",
        ])
        .args(["--disable-slash-commands", "--setting-sources", ""])
        .args(["--system-prompt", request.system])
        .args(["--json-schema", &request.schema.to_string()])
        // Away from any project folder, so nothing of one is read.
        .current_dir(std::env::temp_dir());
    let envelope: Value = if images.is_empty() {
        command.args(["--output-format", "json"]);
        let stdout = run_tool(command, request.user, "Claude Code")?;
        serde_json::from_str(stdout.trim()).context("Claude Code's answer could not be read")?
    } else {
        // Pictures go in as one message of the streamed form, and the
        // answer comes back as a stream whose last record is the result.
        use base64::Engine;
        let mut content: Vec<Value> = Vec::new();
        for image in images {
            let kind = image_type(image)
                .ok_or_else(|| anyhow!("{} is not a picture Claude reads", image.display()))?;
            let bytes = std::fs::read(image)
                .with_context(|| format!("could not read {}", image.display()))?;
            content.push(json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": kind,
                    "data": base64::engine::general_purpose::STANDARD.encode(bytes),
                },
            }));
        }
        content.push(json!({ "type": "text", "text": request.user }));
        let message = json!({
            "type": "user",
            "message": { "role": "user", "content": content },
        });
        command.args([
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
        ]);
        let stdout = run_tool(command, &format!("{message}\n"), "Claude Code")?;
        stdout
            .lines()
            .rev()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|record| record.get("type").and_then(Value::as_str) == Some("result"))
            .ok_or_else(|| anyhow!("Claude Code gave no result"))?
    };
    if envelope.get("is_error").and_then(Value::as_bool) == Some(true) {
        let reason = envelope
            .get("result")
            .and_then(Value::as_str)
            .unwrap_or("it reported an error");
        bail!("Claude Code: {reason}");
    }
    if let Some(structured) = envelope.get("structured_output").filter(|v| v.is_object()) {
        return Ok(structured.to_string());
    }
    envelope
        .get("result")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("Claude Code's answer has no text"))
}

fn codex_cli(model: &str, request: &Request<'_>, images: &[PathBuf]) -> Result<String> {
    let codex = find_codex_cli().ok_or_else(|| anyhow!("Codex is not installed"))?;
    // Requests run side by side: each needs a folder of its own.
    static REQUESTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let scratch = std::env::temp_dir().join(format!(
        "seogi-codex-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
        REQUESTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&scratch).context("could not create a scratch folder")?;
    let schema_path = scratch.join("schema.json");
    let answer_path = scratch.join("answer.txt");
    let result = (|| {
        std::fs::write(&schema_path, request.schema.to_string())?;
        let mut command = Command::new(codex);
        command
            .args(["exec", "--skip-git-repo-check", "--sandbox", "read-only"])
            .arg("--output-schema")
            .arg(&schema_path)
            .arg("--output-last-message")
            .arg(&answer_path)
            .current_dir(&scratch);
        if !model.is_empty() {
            command.args(["--model", model]);
        }
        for image in images {
            command.arg("--image").arg(image);
        }
        command.arg("-");
        let prompt = format!("{}\n\n{}", request.system, request.user);
        run_tool(command, &prompt, "Codex")?;
        std::fs::read_to_string(&answer_path).context("Codex left no answer")
    })();
    if let Err(e) = std::fs::remove_dir_all(&scratch) {
        warn!("Could not remove {}: {e}", scratch.display());
    }
    result
}

/// The model in `list` that `model` names: itself, the first whose id or
/// name has it in it, or else the first, which is the recommended one.
fn listed(model: &str, list: &[ModelChoice]) -> String {
    let wanted = model.trim().to_lowercase();
    if list.is_empty() {
        return model.to_string();
    }
    let named = |choice: &&ModelChoice| {
        !wanted.is_empty()
            && (choice.id.to_lowercase().contains(&wanted)
                || choice.label.to_lowercase().contains(&wanted))
    };
    list.iter()
        .find(|choice| choice.id == model)
        .or_else(|| list.iter().find(named))
        .unwrap_or(&list[0])
        .id
        .clone()
}

/// The models offered for Claude Code, the recommended one first.
pub fn claude_models() -> Vec<ModelChoice> {
    [
        ("claude-opus-5-5", "Opus 5.5"),
        ("claude-sonnet-5-5", "Sonnet 5.5"),
    ]
    .into_iter()
    .map(|(id, label)| ModelChoice {
        id: id.to_string(),
        label: label.to_string(),
    })
    .collect()
}

/// A model the user can pick from a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ModelChoice {
    /// The name the tool takes.
    pub id: String,
    /// The name people know it by.
    pub label: String,
}

/// The models offered for Codex, the recommended one first: the current
/// ones of those Codex lists for the signed-in account (it keeps the list in
/// its own folder), the most capable first. Two well-known ones when Codex
/// has not listed any yet.
pub fn codex_models() -> Vec<ModelChoice> {
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs_home().map(|home| home.join(".codex")));
    let listed = home
        .and_then(|home| std::fs::read_to_string(home.join("models_cache.json")).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .map(|cache| codex_models_in(&cache))
        .unwrap_or_default();
    if !listed.is_empty() {
        return listed;
    }
    [
        ("gpt-6.2-astra", "GPT-6.2 Astra"),
        ("gpt-6.2-sol", "GPT-6.2 Sol"),
    ]
    .into_iter()
    .map(|(id, label)| ModelChoice {
        id: id.to_string(),
        label: label.to_string(),
    })
    .collect()
}

/// The models in Codex's list that it shows people and does not call old
/// ("Legacy", "Previous generation"), the most capable kind (Astra) first,
/// otherwise in Codex's order.
fn codex_models_in(cache: &Value) -> Vec<ModelChoice> {
    let mut listed: Vec<(bool, i64, ModelChoice)> = cache
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|model| model.get("visibility").and_then(Value::as_str) == Some("list"))
        .filter(|model| {
            let about = model
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_lowercase();
            !about.starts_with("legacy") && !about.starts_with("previous")
        })
        .filter_map(|model| {
            let id = model.get("slug").and_then(Value::as_str)?.to_string();
            let label = model
                .get("display_name")
                .and_then(Value::as_str)
                .unwrap_or(&id)
                .replace('-', " ")
                .replacen("GPT ", "GPT-", 1);
            let priority = model
                .get("priority")
                .and_then(Value::as_i64)
                .unwrap_or(i64::MAX);
            let capable = id.contains("astra");
            Some((!capable, priority, ModelChoice { id, label }))
        })
        .collect();
    listed.sort_by_key(|(later, priority, _)| (*later, *priority));
    listed.into_iter().map(|(_, _, model)| model).collect()
}

/// Has Ollama download `model`. Returns when it is there, which for a model
/// of several gigabytes takes a while.
pub fn ollama_pull(base_url: &str, model: &str) -> Result<()> {
    let url = format!("{}/api/pull", base_url.trim_end_matches('/'));
    let body = json!({ "model": model, "stream": false });
    tauri::async_runtime::block_on(async {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(3 * 60 * 60))
            .build()?;
        let response = client.post(&url).json(&body).send().await.map_err(|e| {
            if e.is_connect() {
                anyhow!("Ollama is not running at {base_url}")
            } else {
                anyhow!("the request to Ollama failed: {e}")
            }
        })?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() || text.contains("\"error\"") {
            bail!("Ollama could not download {model}: {}", text.trim());
        }
        Ok(())
    })
}

/// The models an Ollama server has, the largest first, for the settings
/// screen. Empty when it is not running.
pub fn ollama_models(base_url: &str) -> Vec<String> {
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let listed: Result<Value> = tauri::async_runtime::block_on(async {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(5))
            .build()?;
        Ok(client.get(&url).send().await?.json::<Value>().await?)
    });
    let mut models: Vec<(u64, String)> = listed
        .ok()
        .and_then(|value| value.get("models").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|model| {
            let name = model.get("name").and_then(Value::as_str)?.to_string();
            let size = model.get("size").and_then(Value::as_u64).unwrap_or(0);
            Some((size, name))
        })
        .collect();
    // The largest first: on this computer it writes the best minutes.
    models.sort_by_key(|(size, _)| std::cmp::Reverse(*size));
    models.into_iter().map(|(_, name)| name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_found_inside_fences_thinking_and_prose() {
        assert_eq!(parse_json(r#"{"a": 1}"#).unwrap()["a"], 1);
        assert_eq!(parse_json("```json\n{\"a\": 2}\n```").unwrap()["a"], 2);
        assert_eq!(
            parse_json("<think>{\"a\": 0}</think>\n결과입니다: {\"a\": 3} 이상입니다.").unwrap()
                ["a"],
            3
        );
        assert!(parse_json("JSON이 없습니다").is_err());
        assert!(parse_json("[1, 2]").is_err());
    }

    #[test]
    fn context_grows_with_the_request() {
        let schema = json!({});
        let small = Request {
            system: "규칙",
            user: "짧은 글",
            schema: &schema,
        };
        assert_eq!(context_tokens(&small), 16_384);
        let long = "가".repeat(9_000);
        let large = Request {
            system: "규칙",
            user: &long,
            schema: &schema,
        };
        assert_eq!(context_tokens(&large), 32_768);
    }

    #[test]
    fn a_refused_model_is_told_apart_from_other_failures() {
        assert!(refuses_model(
            "Codex failed: ERROR: The 'Astra' model is not supported when using Codex with a ChatGPT account."
        ));
        assert!(refuses_model(
            "Claude Code: There's an issue with the selected model (nonexistent-model). It may not exist or you may not have access to it."
        ));
        assert!(refuses_model(
            "Ollama answered 404 Not Found: model \"gemma9\" not found, try pulling it first"
        ));
        assert!(!refuses_model(
            "Claude Code: You've hit your 5-hour limit · resets 4pm (Asia/Seoul)"
        ));
        assert!(!refuses_model(
            "Codex failed: ERROR: workspace routing discovery failed"
        ));
        assert!(!refuses_model(CUT_OFF));

        assert_eq!(
            trouble_code("Claude Code: You've hit your 5-hour limit · resets 4pm"),
            "summary_limit"
        );
        assert_eq!(
            trouble_code("Codex failed: ERROR: The 'Astra' model is not supported"),
            "summary_model_refused"
        );
        assert_eq!(
            trouble_code("Claude Code is not installed"),
            "summary_model"
        );
        assert_eq!(trouble_code(CUT_OFF), "summary_model");
    }

    #[test]
    fn a_command_line_tool_falls_back_to_its_own_choice_of_model() {
        let chosen = Provider::CodexCli {
            model: "Astra".into(),
        };
        assert_eq!(
            chosen.automatic(),
            Some(Provider::CodexCli {
                model: String::new()
            })
        );
        assert_eq!(chosen.automatic().unwrap().automatic(), None);
        assert_eq!(
            Provider::ClaudeCli {
                model: "opus".into()
            }
            .automatic()
            .map(|p| p.label()),
            Some("claude".to_string())
        );
        // A model on this computer or behind a key has no tool to choose.
        assert_eq!(Provider::default_ollama().automatic(), None);

        assert!(!model_refused(&chosen));
        assert_eq!(in_use(&chosen), chosen);
        REFUSED.lock().unwrap().push(chosen.clone());
        assert!(model_refused(&chosen));
        assert_eq!(in_use(&chosen).label(), "codex");
    }

    #[test]
    fn a_saved_model_becomes_one_from_the_list() {
        let codex_list = [
            ModelChoice {
                id: "gpt-6.2-astra".into(),
                label: "GPT-6.2 Astra".into(),
            },
            ModelChoice {
                id: "gpt-6.2-sol".into(),
                label: "GPT-6.2 Sol".into(),
            },
        ];
        let codex = |model: &str| Provider::CodexCli {
            model: model.into(),
        };
        let claude = |model: &str| Provider::ClaudeCli {
            model: model.into(),
        };
        // The name people see, typed in, is the model it names.
        assert_eq!(codex("Astra").tidied(&codex_list), codex("gpt-6.2-astra"));
        assert_eq!(
            codex("gpt-6.2-sol").tidied(&codex_list),
            codex("gpt-6.2-sol")
        );
        // Nothing, or a name no model has: the recommended one.
        assert_eq!(codex("").tidied(&codex_list), codex("gpt-6.2-astra"));
        assert_eq!(codex("o9").tidied(&codex_list), codex("gpt-6.2-astra"));
        assert_eq!(claude("sonnet").tidied(&[]), claude("claude-sonnet-5-5"));
        assert_eq!(claude("").tidied(&[]), claude("claude-opus-5-5"));
        assert_eq!(claude("haiku").tidied(&[]), claude("claude-opus-5-5"));
        assert_eq!(
            Provider::default_ollama().tidied(&codex_list),
            Provider::default_ollama()
        );
    }

    #[test]
    fn codex_offers_its_current_models_the_most_capable_first() {
        let cache = json!({
            "models": [
                { "slug": "gpt-9-sol", "display_name": "GPT-9-Sol", "description": "Fast, go-to model.", "visibility": "list", "priority": 1 },
                { "slug": "gpt-9-astra", "display_name": "GPT-9-Astra", "description": "Most capable.", "visibility": "list", "priority": 2 },
                { "slug": "hidden", "display_name": "Hidden", "description": "", "visibility": "hide", "priority": 0 },
                { "slug": "gpt-8-sol", "display_name": "GPT-8-Sol", "description": "Legacy model.", "visibility": "list", "priority": 3 },
                { "slug": "gpt-8-astra", "display_name": "GPT-8-Astra", "description": "Previous generation model.", "visibility": "list", "priority": 4 },
            ]
        });
        let models = codex_models_in(&cache);
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["gpt-9-astra", "gpt-9-sol"]
        );
        assert_eq!(models[0].label, "GPT-9 Astra");
        assert!(codex_models_in(&json!({})).is_empty());
    }

    #[test]
    fn only_a_local_endpoint_counts_as_local() {
        assert!(Provider::default_ollama().is_local());
        assert!(!Provider::ClaudeCli {
            model: "sonnet".into()
        }
        .is_local());
        assert!(!Provider::OpenAiCompatible {
            base_url: "https://api.openai.com/v1".into(),
            model: "gpt".into(),
            key: "openai".into()
        }
        .is_local());
    }
}
