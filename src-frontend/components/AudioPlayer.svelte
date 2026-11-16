<script lang="ts">
  import { onDestroy } from "svelte";
  import { convertFileSrc, invoke } from "../lib/tauri";
  import Icon from "./Icon.svelte";

  let {
    audioPath = null,
    bookmarks = [],
    sessionId = null,
    onTimeUpdate = (_ms: number) => {},
    onSeekRequest,
    onRebuilt = (_path: string) => {},
  }: {
    audioPath: string | null;
    bookmarks?: any[];
    sessionId?: string | null;
    onTimeUpdate?: (ms: number) => void;
    onSeekRequest?: { ms: number; autoplay?: boolean } | null;
    onRebuilt?: (path: string) => void;
  } = $props();

  let audioEl: HTMLAudioElement | undefined = $state();
  let playing = $state(false);
  let currentTime = $state(0);
  let duration = $state(0);
  let audioSrc = $derived(audioPath ? convertFileSrc(audioPath) : "");

  function stopPlayback(clearSource = false) {
    if (audioEl) {
      audioEl.pause();
      audioEl.currentTime = 0;
      if (clearSource) {
        audioEl.removeAttribute("src");
        audioEl.load();
      }
    }

    playing = false;
    currentTime = 0;
    duration = 0;
    loadError = null;
  }

  $effect(() => {
    const el = audioEl;
    const src = audioSrc;

    if (!el) return;

    stopPlayback(!src);
    if (src) {
      el.load();
    }
  });

  onDestroy(() => {
    stopPlayback(true);
  });

  function togglePlay() {
    if (!audioEl || !audioSrc) return;
    if (playing) {
      audioEl.pause();
    } else {
      audioEl.play();
    }
  }

  function handleTimeUpdate() {
    if (!audioEl) return;
    currentTime = audioEl.currentTime;
    onTimeUpdate(currentTime * 1000);
  }

  function handleLoadedMetadata() {
    if (!audioEl) return;
    duration = audioEl.duration;
    loadError = null;
  }

  let loadError = $state<string | null>(null);
  let rebuilding = $state(false);
  let rebuildError = $state<string | null>(null);

  function handlePlay() { playing = true; loadError = null; }
  function handlePause() { playing = false; }
  function handleEnded() { playing = false; }
  function handleError() {
    loadError = "Failed to load audio. The file may be missing or in an unsupported format.";
  }

  /** Deletes the cached playback/mix files for this session and rebuilds
   * from the raw source tracks (see `rebuild_session_audio`). */
  async function rebuildAudio() {
    if (!sessionId || rebuilding) return;
    rebuilding = true;
    rebuildError = null;
    try {
      const path: string = await invoke("rebuild_session_audio", { sessionId });
      loadError = null;
      onRebuilt(path);
    } catch (e: any) {
      rebuildError = typeof e === "string" ? e : (e?.message ?? "Rebuild failed");
    }
    rebuilding = false;
  }

  function handleScrub(e: Event) {
    const target = e.target as HTMLInputElement;
    const time = parseFloat(target.value);
    if (audioEl) {
      audioEl.currentTime = time;
      currentTime = time;
      onTimeUpdate(time * 1000);
    }
  }

  /** Seek to a session-time offset. Used by bookmark markers, prev/next
   * bookmark navigation, and external seek requests — always starts
   * playback unless explicitly told not to (e.g. positioning the playhead
   * at the first bookmark when a session is first opened). */
  function seekToMs(ms: number, autoplay = true) {
    if (!audioEl) return;
    const seekTime = ms / 1000;
    audioEl.currentTime = seekTime;
    currentTime = seekTime;
    onTimeUpdate(ms);
    if (autoplay && !playing) {
      audioEl.play();
    }
  }

  // External seek requests (transcript line click, bookmark row click in Archive).
  $effect(() => {
    if (onSeekRequest && audioEl) {
      seekToMs(onSeekRequest.ms, onSeekRequest.autoplay ?? true);
    }
  });

  function fmtTime(secs: number): string {
    if (!isFinite(secs)) return "00:00";
    const m = Math.floor(secs / 60).toString().padStart(2, "0");
    const s = Math.floor(secs % 60).toString().padStart(2, "0");
    return `${m}:${s}`;
  }

  let sortedBookmarks = $derived([...bookmarks].sort((a, b) => a.offset_ms - b.offset_ms));

  // Bookmark positions as percentages for markers on scrubber
  let bookmarkMarkers = $derived(
    duration > 0
      ? sortedBookmarks.map(b => ({
          pct: ((b.offset_ms / 1000) / duration) * 100,
          note: b.note,
          offset_ms: b.offset_ms,
        }))
      : []
  );

  // A little slack so "previous"/"next" don't just re-select the bookmark
  // the playhead is already sitting on.
  const BOOKMARK_NAV_SLACK_MS = 250;

  function prevBookmark() {
    if (sortedBookmarks.length === 0) return;
    const curMs = currentTime * 1000;
    const target = [...sortedBookmarks].reverse().find(b => b.offset_ms < curMs - BOOKMARK_NAV_SLACK_MS);
    seekToMs((target ?? sortedBookmarks[0]).offset_ms);
  }

  function nextBookmark() {
    if (sortedBookmarks.length === 0) return;
    const curMs = currentTime * 1000;
    const target = sortedBookmarks.find(b => b.offset_ms > curMs + BOOKMARK_NAV_SLACK_MS);
    seekToMs((target ?? sortedBookmarks[sortedBookmarks.length - 1]).offset_ms);
  }

  /** `[`/`]` jump between bookmarks while this player (or the Archive page
   * it lives on) is active, as long as the user isn't typing somewhere else. */
  function handleWindowKeydown(e: KeyboardEvent) {
    if (sortedBookmarks.length === 0 || !audioSrc) return;
    const active = document.activeElement as HTMLElement | null;
    const tag = active?.tagName.toLowerCase();
    if (tag === "input" || tag === "textarea" || active?.isContentEditable) return;
    if (e.key === "[") { e.preventDefault(); prevBookmark(); }
    else if (e.key === "]") { e.preventDefault(); nextBookmark(); }
  }
</script>

<svelte:window onkeydown={handleWindowKeydown} />

{#if audioPath}
  <div class="audio-player">
    <!-- Hidden native audio element -->
    <audio
      bind:this={audioEl}
      src={audioSrc}
      ontimeupdate={handleTimeUpdate}
      onloadedmetadata={handleLoadedMetadata}
      onplay={handlePlay}
      onpause={handlePause}
      onended={handleEnded}
      onerror={handleError}
      preload="metadata"
    ></audio>

    <button
      class="player-btn"
      onclick={togglePlay}
      title={playing ? "Pause" : "Play"}
      disabled={!audioSrc}
    >
      <Icon name={playing ? "pause" : "play"} size={16} />
    </button>

    {#if sortedBookmarks.length > 0}
      <button class="player-nav-btn" onclick={prevBookmark} title="Previous bookmark ([)" disabled={!audioSrc}>
        <Icon name="chevron-left" size={14} />
      </button>
    {/if}

    <span class="player-time">{fmtTime(currentTime)}</span>

    <div class="player-scrubber-wrap">
      <input
        type="range"
        class="player-scrubber"
        min="0"
        max={duration || 0}
        step="0.1"
        value={currentTime}
        oninput={handleScrub}
      />
      <!-- Bookmark markers on scrubber: wider invisible hit area around a
           thin visible tick, in a color distinct from the accent-colored
           thumb so they read as a separate layer. -->
      {#each bookmarkMarkers as bm}
        <button
          type="button"
          class="scrubber-bookmark"
          style="left: {bm.pct}%"
          title={bm.note ? `[${fmtTime(bm.offset_ms / 1000)}] ${bm.note}` : `Bookmark at ${fmtTime(bm.offset_ms / 1000)}`}
          onclick={() => seekToMs(bm.offset_ms)}
          aria-label={bm.note ? `Seek to bookmark: ${bm.note}` : "Seek to bookmark"}
        ></button>
      {/each}
    </div>

    {#if sortedBookmarks.length > 0}
      <button class="player-nav-btn" onclick={nextBookmark} title="Next bookmark (])" disabled={!audioSrc}>
        <Icon name="chevron-right" size={14} />
      </button>
    {/if}

    <span class="player-time">{fmtTime(duration)}</span>
  </div>
  {#if loadError}
    <div class="player-error">
      <span>{loadError}</span>
      {#if sessionId}
        <button class="btn-rebuild" onclick={rebuildAudio} disabled={rebuilding}>
          {#if rebuilding}<Icon name="spinner" size={12}/> Rebuilding...{:else}<Icon name="refresh" size={12}/> Rebuild audio{/if}
        </button>
      {/if}
    </div>
    {#if rebuildError}<div class="player-error">{rebuildError}</div>{/if}
  {/if}
{/if}

<style>
  .audio-player {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    background: var(--bg-elevated);
    border-bottom: 1px solid var(--border-subtle);
    flex-shrink: 0;
  }
  .player-btn {
    width: 32px; height: 32px; border: none; border-radius: 50%;
    display: flex; align-items: center; justify-content: center;
    cursor: pointer; transition: transform 0.1s, background 0.1s;
    background: var(--accent); color: white; flex-shrink: 0;
  }
  .player-btn:disabled {
    cursor: not-allowed;
    opacity: 0.65;
    transform: none;
  }
  .player-btn:hover { transform: scale(1.08); }
  .player-btn:active { transform: scale(0.95); }
  .player-nav-btn {
    width: 22px; height: 22px; border: none; border-radius: 50%;
    display: flex; align-items: center; justify-content: center;
    cursor: pointer; background: none; color: var(--text-tertiary);
    flex-shrink: 0; transition: color 0.1s, background 0.1s;
  }
  .player-nav-btn:hover:not(:disabled) { color: var(--text-primary); background: var(--bg-secondary); }
  .player-nav-btn:disabled { cursor: not-allowed; opacity: 0.4; }
  .player-time {
    font-family: "SF Mono", monospace; font-size: 11px;
    color: var(--text-tertiary); white-space: nowrap; min-width: 38px;
    text-align: center;
  }
  .player-scrubber-wrap {
    flex: 1; position: relative; height: 20px;
    display: flex; align-items: center;
  }
  .player-scrubber {
    width: 100%; height: 4px; appearance: none; background: var(--border);
    border-radius: 2px; cursor: pointer; outline: none;
  }
  .player-scrubber::-webkit-slider-thumb {
    appearance: none; width: 12px; height: 12px; border-radius: 50%;
    background: var(--accent); cursor: pointer;
    border: 2px solid var(--bg-primary);
    box-shadow: 0 1px 3px rgba(0,0,0,0.3);
  }
  .scrubber-bookmark {
    position: absolute; top: 50%; transform: translate(-50%, -50%);
    width: 12px; height: 18px; padding: 0; margin: 0; border: none;
    background: transparent; cursor: pointer;
  }
  /* The visible tick, drawn with a border so the wider invisible button
     above still has a comfortable ≥10px hit area. */
  .scrubber-bookmark::before {
    content: "";
    position: absolute; top: 50%; left: 50%; transform: translate(-50%, -50%);
    width: 4px; height: 12px; border-radius: 1px;
    background: var(--warning, #f59e0b);
  }
  .scrubber-bookmark:hover::before { background: var(--danger, #f87171); }
  .player-error {
    display: flex; align-items: center; gap: 10px;
    padding: 4px 16px; font-size: 11px;
    color: var(--warning); background: var(--bg-elevated);
    border-bottom: 1px solid var(--border-subtle);
  }
  .btn-rebuild {
    display: flex; align-items: center; gap: 4px; flex-shrink: 0;
    background: none; border: 1px solid var(--border-subtle); border-radius: var(--radius-sm);
    color: var(--text-secondary); font-size: 11px; cursor: pointer; padding: 2px 8px;
  }
  .btn-rebuild:hover:not(:disabled) { color: var(--text-primary); background: var(--bg-secondary); }
  .btn-rebuild:disabled { opacity: 0.6; cursor: not-allowed; }
</style>
