#!/usr/bin/env bash
# Gravai local-deploy script.
#
# Builds Gravai.app (release, signed) and installs it to /Applications.
# All output is streamed AND captured to a log file so post-mortem is easy
# when something kills the build mid-flight.
#
# Usage:
#   scripts/deploy.sh              # default: 4 parallel cargo jobs
#   JOBS=2 scripts/deploy.sh       # lower parallelism (less RAM)
#   JOBS=8 scripts/deploy.sh       # more parallelism (faster, more RAM)
#   NO_DETACH=1 scripts/deploy.sh  # don't disown the build (for debugging)

set -u
set -o pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

JOBS="${JOBS:-4}"
DEST="/Applications/Gravai.app"
TS="$(date +%Y%m%d-%H%M%S)"
LOGFILE="/tmp/gravai-deploy-${TS}.log"

# Disable Metal shader precompilation (works without full Xcode.app)
export MISTRALRS_METAL_PRECOMPILE="${MISTRALRS_METAL_PRECOMPILE:-0}"
export CARGO_BUILD_JOBS="$JOBS"
if command -v clang >/dev/null 2>&1; then
  CLANG_RT_DIR="$(clang -print-resource-dir 2>/dev/null)/lib/darwin"
  if [ -f "$CLANG_RT_DIR/libclang_rt.osx.a" ]; then
    export LIBRARY_PATH="$CLANG_RT_DIR${LIBRARY_PATH:+:$LIBRARY_PATH}"
  fi
fi
# Force unbuffered Rust output so partial logs survive a SIGKILL
export CARGO_TERM_PROGRESS_WHEN=never
export RUST_LOG_STYLE=never

c_red()    { printf '\033[31m%s\033[0m\n' "$*"; }
c_green()  { printf '\033[32m%s\033[0m\n' "$*"; }
c_yellow() { printf '\033[33m%s\033[0m\n' "$*"; }
c_blue()   { printf '\033[34m%s\033[0m\n' "$*"; }

# NOTE: do NOT name this "log" — that shadows macOS's /usr/bin/log
say() { echo "[deploy $(date +%H:%M:%S)] $*" | tee -a "$LOGFILE"; }

post_mortem() {
  local rc="$1"
  echo
  c_red "❌ Build failed (exit $rc)."
  c_yellow "Full log: $LOGFILE"
  echo
  c_blue "── Last 50 lines of log ────────────────────────────────────"
  tail -n 50 "$LOGFILE" 2>/dev/null
  echo
  c_blue "── Recent kernel jetsam (OOM) events ───────────────────────"
  /usr/bin/log show --predicate 'eventMessage CONTAINS[c] "jetsam"' \
    --last 10m --style compact 2>/dev/null | grep -iE 'cargo|rustc|node|pnpm|gravai|ld' | tail -n 20 \
    || echo "  (no relevant jetsam entries — this was NOT a kernel OOM kill)"
  echo
  c_blue "── Recent SIGKILL events for build tools ───────────────────"
  /usr/bin/log show --predicate '(eventMessage CONTAINS "Killed: 9" OR eventMessage CONTAINS "SIGKILL")' \
    --last 5m --style compact 2>/dev/null | grep -iE 'pnpm|node|cargo|rustc|caffeinate' | tail -n 20 \
    || echo "  (no SIGKILLs in system log)"
  echo
  c_yellow "Suggested next steps:"
  echo "  1. Run from a real Terminal/iTerm window (not Zed task panel):"
  echo "       cd $ROOT && ./scripts/deploy.sh"
  echo "  2. Lower memory pressure:   JOBS=2 ./scripts/deploy.sh"
  echo "  3. Sanity-check pnpm:       pnpm --version && pnpm tauri --version"
  echo "  4. Inspect full log:        less $LOGFILE"
}
trap 'rc=$?; if [ $rc -ne 0 ]; then post_mortem $rc; fi' EXIT

c_blue "════════════════════════════════════════════════════════════"
c_blue " Gravai deploy"
c_blue "  - JOBS=$JOBS"
c_blue "  - log: $LOGFILE"
c_blue "════════════════════════════════════════════════════════════"

# 0. Quick sanity check — fail fast & loud if the toolchain is broken
say "🔎 Sanity check: which/version of pnpm, node, cargo"
{
  echo "PATH=$PATH"
  echo -n "pnpm:    "; command -v pnpm  || echo "MISSING"
  echo -n "node:    "; command -v node  || echo "MISSING"
  echo -n "cargo:   "; command -v cargo || echo "MISSING"
  pnpm  --version 2>&1 | sed 's/^/pnpm  v/'
  node  --version 2>&1 | sed 's/^/node  /'
  cargo --version 2>&1
} | tee -a "$LOGFILE"

# 1. Quit any running Gravai (frees RAM)
say "🛑 Quitting any running Gravai instance…"
osascript -e 'tell application "Gravai" to quit' >/dev/null 2>&1 || true
pkill -x "Gravai"     >/dev/null 2>&1 || true
pkill -x "gravai-app" >/dev/null 2>&1 || true
sleep 1

# 2. Build the Tauri bundle.
#    Detach into its own session so a parent task supervisor (e.g. Zed)
#    cannot signal-kill the whole pipeline. Output is streamed to log.
say "🛠  pnpm tauri build --bundles app   (CARGO_BUILD_JOBS=$JOBS)"
say "    streaming to $LOGFILE"

if [ -z "${NO_DETACH:-}" ]; then
  # Run with nohup so SIGHUP from a parent task supervisor (e.g. Zed)
  # cannot bring the build down. stdin redirected from /dev/null.
  nohup pnpm tauri build --bundles app </dev/null 2>&1 | tee -a "$LOGFILE"
  build_rc=${PIPESTATUS[0]}
else
  pnpm tauri build --bundles app 2>&1 | tee -a "$LOGFILE"
  build_rc=${PIPESTATUS[0]}
fi

if [ "$build_rc" -ne 0 ]; then
  exit "$build_rc"
fi

# 3. Sign with cert-based requirements (TCC permissions persist across updates)
say "🔏 Signing app bundle…"
make sign 2>&1 | tee -a "$LOGFILE"

# 4. Locate the freshly built .app
APP="$(find target/release/bundle/macos -maxdepth 1 -name '*.app' 2>/dev/null | head -1)"
if [ -z "$APP" ]; then
  c_red "❌ Build succeeded but no .app produced under target/release/bundle/macos/"
  exit 1
fi
say "📦 Built bundle: $APP"

# 5. Install to /Applications
if [ -d "$DEST" ]; then
  say "🗑  Removing existing $DEST"
  if ! rm -rf "$DEST"; then
    c_red "❌ Could not remove $DEST"
    c_yellow "    Try:  sudo rm -rf $DEST  &&  ./scripts/deploy.sh"
    exit 1
  fi
fi

say "📥 Copying $APP → $DEST"
cp -R "$APP" "$DEST"

say "🔓 Clearing macOS quarantine…"
xattr -dr com.apple.quarantine "$DEST" 2>/dev/null || true

say "🚀 Launching Gravai…"
open "$DEST"

VERSION="$(grep '^version' Cargo.toml | head -1 | perl -pe 's/version = "(.*)"/$1/; chomp')"
c_green "✅ Deployed Gravai v${VERSION} → /Applications/Gravai.app"
c_blue   "   log: $LOGFILE"
trap - EXIT
exit 0
