<script lang="ts">
  import { tick } from "svelte";
  import { sourceIcon } from "../lib/tauri";

  let { utterances = [], autoScroll = true, showEmotions = true }: { utterances: any[]; autoScroll?: boolean; showEmotions?: boolean } = $props();

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

  const EMOTION_COLORS: Record<string, string> = {
    joy: "var(--success, #34d399)",
    love: "var(--success, #34d399)",
    gratitude: "var(--success, #34d399)",
    optimism: "var(--success, #34d399)",
    excitement: "var(--success, #34d399)",
    anger: "var(--danger, #f87171)",
    disgust: "var(--danger, #f87171)",
    disapproval: "var(--danger, #f87171)",
    sadness: "var(--info, #60a5fa)",
    grief: "var(--info, #60a5fa)",
    disappointment: "var(--info, #60a5fa)",
    fear: "#f59e0b",
    nervousness: "#f59e0b",
    neutral: "var(--text-tertiary)",
  };

  function emotionColor(label: string): string {
    return EMOTION_COLORS[label] ?? "var(--text-tertiary)";
  }

  let el: HTMLElement;
  $effect(() => {
    void utterances.length; // Explicitly track length so effect re-runs when items are added
    if (autoScroll && el) {
      tick().then(() => { el.scrollTop = el.scrollHeight; });
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
        {#if showEmotions && u.sentiment_label}
          <span
            class="emotion-badge"
            style="color: {emotionColor(u.sentiment_label)}"
            title={u.sentiment_score != null ? `${u.sentiment_label} (${(u.sentiment_score * 100).toFixed(0)}%)` : u.sentiment_label}
          >{u.sentiment_label}</span>
        {/if}
      </div>
    {/each}
  {/if}
</div>
