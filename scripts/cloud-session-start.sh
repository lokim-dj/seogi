#!/usr/bin/env bash
# Prepares a Claude Code cloud session (claude.ai/code). It runs at the start
# of every session from .claude/settings.json and does nothing on a computer:
# CLAUDE_CODE_REMOTE is "true" only in the cloud VM.
set -euo pipefail
[ "${CLAUDE_CODE_REMOTE:-}" = "true" ] || exit 0
cd "${CLAUDE_PROJECT_DIR:-$(dirname "$0")/..}"
bun install --frozen-lockfile
cat <<'NOTE'
Cloud session for Seogi:
- Rust tests run on macOS and the Android build needs its SDK: CI ("rust", "android") runs them on the pull request.
- Here, run what Linux can: bun run lint, bun run format:check, bunx tsc --noEmit, bun run check:translations, bun run check:version.
- The name check against the maintainer's private lists cannot run here. Open the pull request and leave it unmerged; it is merged from the maintainer's computer.
NOTE
