// Signing in with a subscription, one line per tool: Claude (Claude Code)
// and ChatGPT (Codex). Each line says where the tool stands on this
// computer and offers the one thing to do next: the installation page when
// it is not here, its own sign-in when it is not signed in. While a sign-in
// or an installation is under way the line looks again every few seconds,
// and again whenever the window comes back to the front, so it changes by
// itself. Settings shows the lines as rows; the first-run screen shows them
// small, inside its table.

import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type SignIn, type SignIns, type Tool } from "@/bindings";
import { NS } from "../i18n";

const TOOLS: Tool[] = ["claude", "codex"];

/** How often a line looks again while a sign-in or installation is open. */
const LOOK_EVERY_MS = 2500;
/** When it stops looking by itself: the window coming back still looks. */
const GIVE_UP_MS = 10 * 60 * 1000;

type Waiting = {
  tool: Tool;
  for: "sign_in" | "install";
  since: number;
  /** Signing in again while signed in: the state does not change, so the
   *  line waits until the window comes back to the front. */
  again?: boolean;
};

export default function SignInRows({
  compact = false,
  onChange,
}: {
  compact?: boolean;
  /** Called with every new reading, so the screen can follow it. */
  onChange?: (signIns: SignIns) => void;
}) {
  const { t } = useTranslation(NS);
  const [signIns, setSignIns] = useState<SignIns | null>(null);
  const [waiting, setWaiting] = useState<Waiting | null>(null);
  const [trouble, setTrouble] = useState<Tool | null>(null);
  const looking = useRef(false);
  const told = useRef(onChange);
  told.current = onChange;

  const look = useCallback(async () => {
    if (looking.current) return;
    looking.current = true;
    try {
      const result = await commands.seogiSignIns();
      const data = result.status === "ok" ? result.data : null;
      if (data) {
        setSignIns(data);
        told.current?.(data);
      }
      setWaiting((now) => {
        if (!now) return now;
        if (Date.now() - now.since > GIVE_UP_MS) return null;
        if (!data) return now;
        const state = data[now.tool];
        const done =
          now.for === "sign_in"
            ? state === "signed_in" && !now.again
            : state !== "missing";
        return done ? null : now;
      });
    } finally {
      looking.current = false;
    }
  }, []);

  useEffect(() => {
    void look();
    const back = () => {
      setWaiting((now) => (now?.again ? null : now));
      void look();
    };
    window.addEventListener("focus", back);
    return () => window.removeEventListener("focus", back);
  }, [look]);

  useEffect(() => {
    if (!waiting) return;
    const timer = window.setInterval(() => void look(), LOOK_EVERY_MS);
    return () => window.clearInterval(timer);
  }, [waiting, look]);

  const signIn = async (tool: Tool) => {
    setTrouble(null);
    const result = await commands.seogiSignIn(tool, t("signIn.farewell"));
    if (result.status === "error") {
      setTrouble(tool);
      void look();
      return;
    }
    setWaiting({
      tool,
      for: "sign_in",
      since: Date.now(),
      again: signIns?.[tool] === "signed_in",
    });
  };

  const install = async (tool: Tool) => {
    setTrouble(null);
    const result = await commands.seogiOpenInstallPage(tool);
    if (result.status === "error") {
      setTrouble(tool);
      return;
    }
    setWaiting({ tool, for: "install", since: Date.now() });
  };

  const name = (tool: Tool) => t(`signIn.name.${tool}`);

  /** "로그인됨" quietly, or what is missing. */
  const stateText = (state: SignIn | undefined, tool: Tool) => {
    if (!state) return null;
    if (state === "signed_in")
      return <span className="t2">{t("signIn.signedIn")}</span>;
    if (state === "signed_out")
      return <span className="t3">{t("signIn.signedOut")}</span>;
    return <span className="t3">{t(`signIn.missing.${tool}`)}</span>;
  };

  /** What the line says under its name: what will happen, or is. */
  const hint = (state: SignIn | undefined, tool: Tool) => {
    if (trouble === tool)
      return t(state === "missing" ? "signIn.pageFailed" : "signIn.failed");
    if (waiting?.tool === tool)
      return t(
        waiting.for === "sign_in" ? "signIn.waiting" : "signIn.waitingInstall",
      );
    if (state === "signed_in") return t("signIn.signedInHint");
    if (state === "signed_out") return t("signIn.signedOutHint");
    if (state === "missing") return t(`signIn.missingHint.${tool}`);
    return "";
  };

  const action = (state: SignIn | undefined, tool: Tool) => {
    if (!state) return null;
    const className = compact ? "key key-small" : "btn";
    if (state === "missing")
      return (
        <button
          type="button"
          className={className}
          onClick={() => void install(tool)}
        >
          {t("signIn.download")}
        </button>
      );
    return (
      <button
        type="button"
        className={className}
        onClick={() => void signIn(tool)}
      >
        {t(state === "signed_in" ? "signIn.again" : "signIn.login")}
      </button>
    );
  };

  if (compact) {
    return (
      <div className="flex flex-col gap-2 mt-2">
        {TOOLS.map((tool) => {
          const state = signIns?.[tool];
          const line = hint(state, tool);
          return (
            <div key={tool}>
              <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
                <span className="font-normal">{name(tool)}</span>
                {stateText(state, tool)}
                {action(state, tool)}
              </div>
              {waiting?.tool === tool || trouble === tool ? (
                <div
                  className={`text-[13px] ${trouble === tool ? "rt" : "t3"}`}
                >
                  {line}
                </div>
              ) : null}
            </div>
          );
        })}
      </div>
    );
  }

  return (
    <>
      {TOOLS.map((tool) => {
        const state = signIns?.[tool];
        return (
          <div key={tool} className="srow">
            <div>
              <div className="l">{t("signIn.with", { name: name(tool) })}</div>
              <div className={`h ${trouble === tool ? "!text-rt" : ""}`}>
                {hint(state, tool)}
              </div>
            </div>
            <div className="v">
              {stateText(state, tool)}
              {action(state, tool)}
            </div>
          </div>
        );
      })}
    </>
  );
}
