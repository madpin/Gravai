<script lang="ts">
  import { sourceIcon } from "../lib/tauri";

  let { utterances = [], autoScroll = true }: { utterances: any[]; autoScroll?: boolean } = $props();

  const speakerColors = ["#7c6cff", "#34d399", "#fbbf24", "#f87171", "#60a5fa", "#a78bfa", "#fb923c", "#2dd4bf"];
  let speakerColorMap: Record<string, string> = {};
  let colorIdx = 0;

  function getSpeakerColor(speaker: string): string {
    if (!speakerColorMap[speaker]) {
      speakerColorMap[speaker] = speakerColors[colorIdx % speakerColors.length];
      colorIdx++;
    }
    return speakerColorMap[speaker];
  }

  function fmtTime(ts: string): string {
    try { return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" }); }
    catch { return ""; }
  }

  let el: HTMLElement;
  $effect(() => {
    // Re-run when utterances change
    utterances;
    if (autoScroll && el) {
      requestAnimationFrame(() => { el.scrollTop = el.scrollHeight; });
    }
  });
</script>

<div class="transcript-panel" bind:this={el}>
  {#if utterances.length === 0}
    <div class="empty-state">No transcript yet.</div>
  {:else}
    {#each utterances as u}
      <div class="transcript-line">
        <span class="transcript-meta">
          {sourceIcon(u.source)} {fmtTime(u.timestamp)}
          {#if u.speaker}
            <span class="speaker-tag" style="color: {getSpeakerColor(u.speaker)}">{u.speaker}</span>
          {/if}
        </span>
        <span class="transcript-text" class:low-confidence={u.confidence != null && u.confidence < 0.5}>{u.text}</span>
      </div>
    {/each}
  {/if}
</div>
