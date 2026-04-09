<script lang="ts">
  import { tick } from "svelte";
  import { invoke, sourceIconName } from "../lib/tauri";
  import Icon from "./Icon.svelte";

  let { utterances = [], autoScroll = true, showEmotions = true, sessionId = null }: { utterances: any[]; autoScroll?: boolean; showEmotions?: boolean; sessionId?: string | null } = $props();

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

  // Track which utterances are showing original (pre-correction) text
  let showingOriginal = $state(new Set<number>());

  function toggleOriginal(id: number) {
    const next = new Set(showingOriginal);
    if (next.has(id)) { next.delete(id); } else { next.add(id); }
    showingOriginal = next;
  }

  // ── Speaker rename ─────────────────────────────────────────────────────────

  // Optimistic local overrides: old speaker label → new display name.
  // Cleared when the parent pushes fresh utterances with the updated speaker value.
  let speakerOverrides = $state<Record<string, string>>({});

  let editingUtteranceId = $state<number | null>(null);
  let editingSpeakerOriginal = $state("");
  let editValue = $state("");
  let suggestions = $state<string[]>([]);
  let selectedSuggestionIdx = $state(-1);
  let renameError = $state<string | null>(null);

  let filteredSuggestions = $derived(
    editValue.trim() === ""
      ? suggestions
      : suggestions.filter(s => s.toLowerCase().includes(editValue.toLowerCase()))
  );

  function getDisplaySpeaker(speaker: string): string {
    return speakerOverrides[speaker] ?? speaker;
  }

  async function startEdit(utteranceId: number, speaker: string) {
    if (!sessionId) return;
    editingUtteranceId = utteranceId;
    editingSpeakerOriginal = speaker;
    editValue = speaker;
    selectedSuggestionIdx = -1;
    renameError = null;
    try {
      const all: string[] = await invoke("get_speaker_suggestions");
      suggestions = all.filter(s => s !== speaker);
    } catch (_) {
      suggestions = [];
    }
  }

  function cancelEdit() {
    editingUtteranceId = null;
    editingSpeakerOriginal = "";
    editValue = "";
    suggestions = [];
    selectedSuggestionIdx = -1;
    renameError = null;
  }

  async function confirmRename(newName?: string) {
    const name = (newName ?? editValue).trim();
    if (!name || !sessionId || !editingSpeakerOriginal) { cancelEdit(); return; }
    if (name === editingSpeakerOriginal) { cancelEdit(); return; }
    try {
      await invoke("rename_speaker_in_session", { sessionId, oldSpeaker: editingSpeakerOriginal, newSpeaker: name });
      // Transfer color so the tag keeps its colour after rename
      if (speakerColorMap[editingSpeakerOriginal] && !speakerColorMap[name]) {
        speakerColorMap[name] = speakerColorMap[editingSpeakerOriginal];
      }
      speakerOverrides[editingSpeakerOriginal] = name;
      cancelEdit();
    } catch (e) {
      renameError = String(e);
    }
  }

  function handleEditKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      const suggestion = filteredSuggestions[selectedSuggestionIdx];
      confirmRename(suggestion ?? undefined);
    } else if (e.key === "Escape") {
      e.preventDefault();
      cancelEdit();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      selectedSuggestionIdx = Math.min(selectedSuggestionIdx + 1, filteredSuggestions.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      selectedSuggestionIdx = Math.max(selectedSuggestionIdx - 1, -1);
    }
  }

  function autoFocus(node: HTMLElement) {
    tick().then(() => { node.focus(); (node as HTMLInputElement).select?.(); });
    return {};
  }
</script>

<div class="transcript-panel" bind:this={el}>
  {#if utterances.length === 0}
    <div class="empty-state">No transcript yet.</div>
  {:else}
    {#each utterances as u}
      <div class="transcript-line">
        <span class="transcript-meta">
          <Icon name={sourceIconName(u.source)} size={12}/> {fmtTime(u.timestamp)}
          {#if u.speaker}
            {@const display = getDisplaySpeaker(u.speaker)}
            {#if editingUtteranceId === u.id}
              <span class="speaker-rename-wrapper">
                <!-- svelte-ignore a11y_autofocus -->
                <input
                  type="text"
                  class="speaker-rename-input"
                  style="color: {getSpeakerColor(display)}"
                  bind:value={editValue}
                  onkeydown={handleEditKeydown}
                  onblur={cancelEdit}
                  use:autoFocus
                  maxlength={80}
                />
                {#if filteredSuggestions.length > 0}
                  <ul class="speaker-suggestions">
                    {#each filteredSuggestions as s, i}
                      <li
                        class:selected={i === selectedSuggestionIdx}
                        onmousedown={(e) => { e.preventDefault(); confirmRename(s); }}
                      >{s}</li>
                    {/each}
                  </ul>
                {/if}
                {#if renameError}
                  <span class="rename-error">{renameError}</span>
                {/if}
              </span>
            {:else}
              <span
                class="speaker-tag"
                class:renameable={!!sessionId}
                style="color: {getSpeakerColor(display)}"
                ondblclick={sessionId ? () => startEdit(u.id, display) : undefined}
                title={sessionId ? "Double-click to rename speaker" : undefined}
              >{display}</span>
            {/if}
          {/if}
        </span>
        <span
          class="transcript-text"
          class:low-confidence={u.confidence != null && u.confidence < 0.5}
        >{#if u.corrected_text && showingOriginal.has(u.id)}<span class="original-text">{u.text}</span>{:else}{u.corrected_text ?? u.text}{/if}{#if u.corrected_text}<button
            class="correction-mark"
            class:showing-original={showingOriginal.has(u.id)}
            data-tooltip={showingOriginal.has(u.id) ? "Showing original — click to restore correction" : `Original: ${u.text}`}
            onclick={() => toggleOriginal(u.id)}
            title=""
          ><Icon name={showingOriginal.has(u.id) ? "corner-up-left" : "pencil"} size={13}/></button>{/if}</span>
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
