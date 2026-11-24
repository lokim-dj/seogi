// The three ways minutes can be written, as the screens offer them.

import type { Provider, Readiness } from "@/bindings";

export type Mode = "local" | "login" | "key";

export function modeOf(provider: Provider): Mode {
  if (provider.kind === "ollama") return "local";
  if (provider.kind === "claude_cli" || provider.kind === "codex_cli") {
    return "login";
  }
  return "key";
}

export const LOCAL: Provider = {
  kind: "ollama",
  base_url: "http://localhost:11434",
  model: "gemma3:12b",
};

export type LoginTool = "claude" | "codex";

/** A signed-in tool with its recommended model, the first it lists. */
export function loginProvider(
  tool: LoginTool,
  readiness?: Readiness | null,
): Provider {
  return tool === "claude"
    ? {
        kind: "claude_cli",
        model: readiness?.claude_models[0]?.id ?? "claude-opus-5-5",
      }
    : {
        kind: "codex_cli",
        model: readiness?.codex_models[0]?.id ?? "gpt-6.2-astra",
      };
}

export type KeyOwner = "openai" | "anthropic" | "gemini";

/** Whose key, the name it is kept under, and the model first offered. */
export const KEY_OWNERS: {
  id: KeyOwner;
  label: string;
  provider: (model: string) => Provider;
  model: string;
}[] = [
  {
    id: "anthropic",
    label: "Anthropic (Claude)",
    model: "claude-sonnet-4-5",
    provider: (model) => ({ kind: "anthropic", model, key: "anthropic" }),
  },
  {
    id: "openai",
    label: "OpenAI",
    model: "gpt-4.1-mini",
    provider: (model) => ({
      kind: "open_ai_compatible",
      base_url: "https://api.openai.com/v1",
      model,
      key: "openai",
    }),
  },
  {
    id: "gemini",
    label: "Google Gemini",
    model: "gemini-2.5-flash",
    provider: (model) => ({
      kind: "open_ai_compatible",
      base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
      model,
      key: "gemini",
    }),
  },
];

export function keyOwnerOf(provider: Provider): KeyOwner {
  if (provider.kind === "anthropic") return "anthropic";
  if (provider.kind === "open_ai_compatible" && provider.key === "gemini") {
    return "gemini";
  }
  return "openai";
}

/** A model to pick: the name the tool takes, and the one people see. */
export interface ModelOption {
  id: string;
  label: string;
  /** What it is good at, as a text of the screens. */
  aboutKey: string;
  recommended: boolean;
}

/** What a model is good at, by the kind its name says it is. */
function aboutKey(id: string): string {
  const kinds = ["opus", "sonnet", "astra", "sol", "terra", "luna"];
  const kind = kinds.find((name) => id.toLowerCase().includes(name));
  return kind ? `settings.models.${kind}` : "";
}

/**
 * The models offered for `provider`, the recommended one first. Only names
 * the tool takes are offered; a saved one that is not among them (an
 * Ollama model since removed) stays, marked, so the screen shows what is
 * saved.
 */
export function modelOptions(
  provider: Provider,
  readiness: Readiness | null,
): ModelOption[] {
  let options: ModelOption[];
  switch (provider.kind) {
    case "claude_cli":
    case "codex_cli":
      options = (
        (provider.kind === "claude_cli"
          ? readiness?.claude_models
          : readiness?.codex_models) ?? []
      ).map((model, index) => ({
        id: model.id,
        label: model.label,
        aboutKey: aboutKey(model.id),
        recommended: index === 0,
      }));
      break;
    case "ollama":
      // The largest first: it writes the best minutes.
      options = (readiness?.ollama_models ?? []).map((name, index) => ({
        id: name,
        label: name,
        aboutKey: "settings.models.local",
        recommended: index === 0,
      }));
      break;
    default: {
      const owner = KEY_OWNERS.find((item) => item.id === keyOwnerOf(provider));
      options = owner
        ? [
            {
              id: owner.model,
              label: owner.model,
              aboutKey: "settings.models.key",
              recommended: true,
            },
          ]
        : [];
    }
  }
  const saved = provider.model;
  const listed = options.some(
    (option) => option.id === saved || option.id === `${saved}:latest`,
  );
  if (saved && !listed && readiness) {
    options.push({
      id: saved,
      label: saved,
      aboutKey:
        provider.kind === "ollama"
          ? "settings.models.notInstalled"
          : "settings.models.notListed",
      recommended: false,
    });
  }
  return options;
}

/** The option that stands for what is saved. */
export function selectedModel(provider: Provider, options: ModelOption[]) {
  const latest = `${provider.model}:latest`;
  return (
    options.find((option) => option.id === provider.model)?.id ??
    options.find((option) => option.id === latest)?.id ??
    provider.model
  );
}
