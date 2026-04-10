#!/usr/bin/env bash
# Regenerates Gravai skill docs after source file edits.
# Runs asynchronously — won't block Claude's workflow.

set -euo pipefail

PROJECT="$(pwd)"
INPUT="$(cat)"
FILE_PATH="$(echo "$INPUT" | jq -r '.tool_input.file_path // empty' 2>/dev/null)"

# Skip if no file path
[[ -z "$FILE_PATH" ]] && exit 0

# Only process files in this project
[[ "$FILE_PATH" != "$PROJECT/"* ]] && exit 0

# Skip edits to skill files themselves (prevent infinite loop)
[[ "$FILE_PATH" == *"/.claude/commands/"* ]] && exit 0
[[ "$FILE_PATH" == *"/.claude/hooks/"* ]] && exit 0

# Only process Rust and Svelte/TS source files
REL="${FILE_PATH#$PROJECT/}"
[[ "$REL" != *.rs ]] && [[ "$REL" != *.svelte ]] && [[ "$REL" != *.ts ]] && exit 0

# Skip test files
[[ "$REL" == */tests/* ]] && exit 0
[[ "$REL" == *_test.rs ]] && exit 0

# Map relative path to skill name
SKILL=""
case "$REL" in
  crates/gravai-core/*)
    SKILL="gravai-core" ;;
  crates/gravai-audio/*)
    SKILL="gravai-audio" ;;
  crates/gravai-transcription/*|crates/gravai-models/*)
    SKILL="gravai-models" ;;
  crates/gravai-intelligence/*)
    SKILL="gravai-intelligence" ;;
  crates/gravai-config/*)
    SKILL="gravai-config" ;;
  crates/gravai-storage/*)
    SKILL="gravai-storage" ;;
  src-tauri/src/commands/session.rs|src-tauri/src/commands/audio.rs)
    SKILL="gravai-session" ;;
  src-tauri/src/lib.rs)
    SKILL="gravai-events" ;;
  src-frontend/src/*)
    SKILL="gravai-frontend" ;;
esac

[[ -z "$SKILL" ]] && exit 0

SKILL_FILE=".claude/commands/${SKILL}.md"

claude --dangerously-skip-permissions -p \
"The file '$REL' in the Gravai project was just modified.

Read its current content, then read '$SKILL_FILE'.

If there are meaningful structural changes — new or removed public types, struct fields, function signatures, trait implementations, modules, or significant behavioral changes — update the skill file to reflect them. Keep the exact same format, headings, and structure; only update the facts that changed.

If the change is minor (comments, docstrings, internal implementation details, formatting), exit without modifying anything." \
2>/dev/null

exit 0
