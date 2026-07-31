#!/usr/bin/env bash
# Gravai session compactor.
#
# Compresses each session's raw multi-track WAV recordings into a single
# compact `playback.m4a` (AAC), then optionally deletes the raw WAV files to
# reclaim disk space. Transcripts, summaries and speaker labels are NOT
# touched — they live in ~/.gravai/gravai.db, independent of the audio.
#
# Why: raw tracks are stored uncompressed 32-bit-float stereo (~1.4 GB/hour
# per track, and each session keeps mic + system + master). The AAC playback
# file is ~50-60 MB/hour — a 20-60x reduction. The playback.m4a is exactly the
# file the app itself uses for in-session playback (get_session_audio_path).
#
# Safety:
#   * A raw WAV is deleted ONLY when a playback.m4a exists AND its duration
#     matches the source recording (within tolerance). Verification failures
#     leave the originals untouched.
#   * Overflow-corrupted WAVs (>4 GB, past the 32-bit WAV size limit have an
#     unreliable header) are detected and reported; only their readable portion
#     can be preserved into playback.m4a — the unreadable tail is not valid,
#     addressable audio and cannot be recovered by any player.
#   * Default mode is COMPRESS-ONLY (purely additive / reversible). Pass
#     --delete to remove raw originals after successful verification.
#
# Usage:
#   scripts/compact_sessions.sh                    # compress all, keep originals (preview)
#   scripts/compact_sessions.sh --only <id>        # process a single session
#   scripts/compact_sessions.sh --delete           # compress + delete raw WAVs (reclaim)
#   scripts/compact_sessions.sh --only <id> --delete
#   BITRATE=96k scripts/compact_sessions.sh        # override AAC bitrate (default 128k)
#   GRAVAI_SESSIONS_DIR=/path scripts/compact_sessions.sh

set -u
set -o pipefail

SESSIONS_DIR="${GRAVAI_SESSIONS_DIR:-$HOME/.gravai/sessions}"
BITRATE="${BITRATE:-128k}"
DELETE=0
ONLY=""

while [ $# -gt 0 ]; do
  case "$1" in
    --delete) DELETE=1 ;;
    --only) shift; ONLY="${1:-}" ;;
    --only=*) ONLY="${1#--only=}" ;;
    -h|--help) sed -n '2,32p' "$0"; exit 0 ;;
    *) echo "Unknown arg: $1" >&2; exit 2 ;;
  esac
  shift
done

command -v ffmpeg  >/dev/null || { echo "ffmpeg not found (brew install ffmpeg)" >&2; exit 1; }
command -v ffprobe >/dev/null || { echo "ffprobe not found (brew install ffmpeg)" >&2; exit 1; }
[ -d "$SESSIONS_DIR" ] || { echo "Sessions dir not found: $SESSIONS_DIR" >&2; exit 1; }

WAV_LIMIT=4294967296   # 2^32, hard ceiling of the WAV 32-bit size field

bytes() { stat -f%z "$1" 2>/dev/null || echo 0; }
dur()   { ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 "$1" 2>/dev/null; }
human() { awk -v b="$1" 'BEGIN{ if(b>=1073741824)printf "%.1f GB",b/1073741824; else if(b>=1048576)printf "%.0f MB",b/1048576; else printf "%d KB",b/1024 }'; }

# globals for the summary
TOTAL_RAW=0        # bytes of raw WAV considered for removal
TOTAL_M4A=0        # bytes of resulting compact playback files
TOTAL_FREED=0      # bytes actually deleted (delete mode)
N_DONE=0; N_SKIP=0; N_FAIL=0; N_CORRUPT=0

process_session() {
  local S="$1" id; id="$(basename "$S")"

  # sources to mix = the real capture tracks; fall back to master if that is all we have
  local sources=()
  [ -f "$S/mic.wav" ]    && sources+=("$S/mic.wav")
  [ -f "$S/system.wav" ] && sources+=("$S/system.wav")
  if [ ${#sources[@]} -eq 0 ] && [ -f "$S/master.wav" ]; then
    sources+=("$S/master.wav")
  fi

  # raw wavs that would be deleted: capture tracks, the redundant master mix,
  # and any leftover derived/temp wavs.
  local raws=() f
  for f in mic.wav system.wav master.wav _mixed_temp.wav _playback_temp.wav playback.wav; do
    [ -f "$S/$f" ] && raws+=("$S/$f")
  done
  for f in "$S"/*trimmed*.wav; do [ -f "$f" ] && raws+=("$f"); done

  if [ ${#sources[@]} -eq 0 ]; then
    # nothing to compress (e.g. only a playback.m4a already present)
    return
  fi

  local raw_total=0
  for f in "${raws[@]}"; do raw_total=$((raw_total + $(bytes "$f"))); done

  # corruption: any source physically bigger than the WAV limit => header wrapped
  local corrupt=0
  for f in "${sources[@]}"; do
    [ "$(bytes "$f")" -ge "$WAV_LIMIT" ] && corrupt=1
  done

  local pb="$S/playback.m4a" note=""
  [ $corrupt -eq 1 ] && { note=" [CORRUPT src: readable portion only]"; N_CORRUPT=$((N_CORRUPT+1)); }

  # longest source duration (from header) for verification
  local maxdur=0 d
  for f in "${sources[@]}"; do
    d="$(dur "$f")"; [ -z "$d" ] && d=0
    awk -v a="$d" -v b="$maxdur" 'BEGIN{exit !(a>b)}' && maxdur="$d"
  done

  # (re)generate playback.m4a unless a valid one already exists
  local need=1
  if [ -f "$pb" ] && [ "$(bytes "$pb")" -gt 10000 ] && [ -n "$(dur "$pb")" ]; then
    need=0
  fi

  if [ $need -eq 1 ]; then
    local tmp="${pb%.m4a}.building.m4a"; rm -f "$tmp"
    # -ac 2 normalises odd/mono channel layouts (e.g. "1 channels (FL)" which the
    # AAC encoder rejects) to standard stereo before encoding.
    if [ ${#sources[@]} -eq 1 ]; then
      ffmpeg -y -hide_banner -loglevel error -i "${sources[0]}" \
        -ac 2 -c:a aac -b:a "$BITRATE" -movflags +faststart "$tmp"
    else
      local inputs=() labels="" i=0
      for f in "${sources[@]}"; do inputs+=(-i "$f"); labels+="[$i:a]"; i=$((i+1)); done
      ffmpeg -y -hide_banner -loglevel error "${inputs[@]}" \
        -filter_complex "${labels}amix=inputs=${#sources[@]}:duration=longest:normalize=0,alimiter=limit=0.95" \
        -ac 2 -c:a aac -b:a "$BITRATE" -movflags +faststart "$tmp"
    fi
    if [ ! -s "$tmp" ]; then
      echo "  FAIL  $id — encode produced no output (originals kept)$note"
      rm -f "$tmp"; N_FAIL=$((N_FAIL+1)); return
    fi
    # verify duration within max(2s, 1%)
    local od; od="$(dur "$tmp")"; [ -z "$od" ] && od=0
    if ! awk -v m="$maxdur" -v o="$od" 'BEGIN{t=m*0.01; if(t<2)t=2; d=o-m; if(d<0)d=-d; exit !(d<=t)}'; then
      echo "  FAIL  $id — duration mismatch (src ${maxdur}s vs out ${od}s), originals kept$note"
      rm -f "$tmp"; N_FAIL=$((N_FAIL+1)); return
    fi
    mv "$tmp" "$pb"
  fi

  local pb_bytes; pb_bytes="$(bytes "$pb")"

  local action="compressed, originals KEPT"
  if [ $DELETE -eq 1 ]; then
    # Final safety gate: only delete raw WAVs when playback.m4a exists and its
    # duration matches the source recording. Re-verified here even for
    # pre-existing playback files (not just freshly encoded ones).
    local pbdur; pbdur="$(dur "$pb")"; [ -z "$pbdur" ] && pbdur=0
    if ! awk -v m="$maxdur" -v o="$pbdur" 'BEGIN{t=m*0.01; if(t<2)t=2; d=o-m; if(d<0)d=-d; exit !(d<=t)}'; then
      echo "  SKIP  $id — playback.m4a ${pbdur}s vs source ${maxdur}s mismatch; originals kept$note"
      N_SKIP=$((N_SKIP+1)); return
    fi
    for f in "${raws[@]}"; do rm -f "$f"; done
    TOTAL_FREED=$((TOTAL_FREED + raw_total))
    action="raws DELETED"
  fi

  TOTAL_RAW=$((TOTAL_RAW + raw_total))
  TOTAL_M4A=$((TOTAL_M4A + pb_bytes))
  N_DONE=$((N_DONE+1))

  printf "  ok    %s  raw=%s -> m4a=%s (%ss, %d src)  [%s]%s\n" \
    "$id" "$(human "$raw_total")" "$(human "$pb_bytes")" "${maxdur%.*}" "${#sources[@]}" "$action" "$note"
}

echo "Gravai session compactor"
echo "  sessions dir : $SESSIONS_DIR"
echo "  bitrate      : $BITRATE"
echo "  mode         : $([ $DELETE -eq 1 ] && echo 'COMPRESS + DELETE raw WAVs' || echo 'compress only (originals kept)')"
[ -n "$ONLY" ] && echo "  only         : $ONLY"
echo

if [ -n "$ONLY" ]; then
  process_session "$SESSIONS_DIR/$ONLY"
else
  for S in "$SESSIONS_DIR"/*/; do
    [ -d "$S" ] || continue
    process_session "${S%/}"
  done
fi

echo
echo "Summary:"
echo "  sessions compacted : $N_DONE   (failed: $N_FAIL, skipped: $N_SKIP, corrupt-src: $N_CORRUPT)"
echo "  raw WAV considered : $(human "$TOTAL_RAW")"
echo "  compact m4a total  : $(human "$TOTAL_M4A")"
if [ $DELETE -eq 1 ]; then
  echo "  SPACE RECLAIMED    : $(human "$TOTAL_FREED")"
else
  echo "  projected reclaim  : $(human "$TOTAL_RAW")   (run again with --delete)"
fi
