<script lang="ts">
  import { onDestroy } from "svelte";
  import { convertFileSrc } from "../lib/tauri";
  import Icon from "./Icon.svelte";

  let {
    audioPath = null,
    bookmarks = [],
    onTimeUpdate = (_ms: number) => {},
    onSeekRequest,
  }: {
    audioPath: string | null;
    bookmarks?: any[];
    onTimeUpdate?: (ms: number) => void;
    onSeekRequest?: { ms: number } | null;
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

  function handlePlay() { playing = true; loadError = null; }
  function handlePause() { playing = false; }
  function handleEnded() { playing = false; }
  function handleError() {
    loadError = "Failed to load audio. The file may be missing or in an unsupported format.";
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

  // External seek requests
  $effect(() => {
    if (onSeekRequest && audioEl) {
      const seekTime = onSeekRequest.ms / 1000;
      audioEl.currentTime = seekTime;
      currentTime = seekTime;
      if (!playing) {
        audioEl.play();
      }
    }
  });

  function fmtTime(secs: number): string {
    if (!isFinite(secs)) return "00:00";
    const m = Math.floor(secs / 60).toString().padStart(2, "0");
    const s = Math.floor(secs % 60).toString().padStart(2, "0");
    return `${m}:${s}`;
  }

  // Bookmark positions as percentages for markers on scrubber
  let bookmarkMarkers = $derived(
    duration > 0
      ? bookmarks.map(b => ({
          pct: ((b.offset_ms / 1000) / duration) * 100,
          note: b.note,
          offset_ms: b.offset_ms,
        }))
      : []
  );
</script>

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
      <!-- Bookmark markers on scrubber -->
      {#each bookmarkMarkers as bm}
        <div
          class="scrubber-bookmark"
          style="left: {bm.pct}%"
          title={bm.note ? `[${fmtTime(bm.offset_ms / 1000)}] ${bm.note}` : `Bookmark at ${fmtTime(bm.offset_ms / 1000)}`}
        ></div>
      {/each}
    </div>

    <span class="player-time">{fmtTime(duration)}</span>
  </div>
  {#if loadError}
    <div class="player-error">{loadError}</div>
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
    width: 3px; height: 12px; background: var(--accent);
    border-radius: 1px; opacity: 0.6; pointer-events: none;
  }
  .player-error {
    padding: 4px 16px; font-size: 11px;
    color: var(--warning); background: var(--bg-elevated);
    border-bottom: 1px solid var(--border-subtle);
  }
</style>
