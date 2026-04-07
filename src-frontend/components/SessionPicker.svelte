<script lang="ts">
  import { fmtDuration } from "../lib/tauri";

  let {
    sessions = [],
    selected = null,
    onselect = (_id: string | null) => {},
  }: {
    sessions: any[];
    selected: string | null;
    onselect: (id: string | null) => void;
  } = $props();

  let query = $state("");
  let open = $state(false);
  let inputEl: HTMLInputElement;

  function formatDate(isoStr: string): string {
    if (!isoStr) return "";
    try {
      return new Date(isoStr).toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
    } catch { return ""; }
  }

  function filteredSessions() {
    const q = query.toLowerCase().trim();
    return sessions.filter(s => {
      if (!q) return true;
      const title = (s.title || s.id || "").toLowerCase();
      const app = (s.meeting_app || "").toLowerCase();
      const date = formatDate(s.started_at).toLowerCase();
      return title.includes(q) || app.includes(q) || date.includes(q);
    });
  }

  function select(id: string | null, name: string) {
    onselect(id);
    query = id ? name : "";
    open = false;
  }

  function handleBlur() {
    setTimeout(() => { open = false; }, 200);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") { open = false; inputEl?.blur(); }
  }

  let displayName = $derived(
    selected
      ? (sessions.find(s => s.id === selected)?.title || selected.slice(0, 8))
      : ""
  );
</script>

<div class="session-picker">
  <input
    bind:this={inputEl}
    class="session-picker-input"
    type="text"
    placeholder={selected ? displayName : "All meetings"}
    bind:value={query}
    onfocus={() => open = true}
    onblur={handleBlur}
    onkeydown={handleKeydown}
  />
  {#if selected && !open}
    <button class="session-picker-clear" onclick={() => select(null, "")} title="Show all meetings">&times;</button>
  {/if}

  {#if open}
    <div class="session-picker-dropdown">
      <div
        class="session-picker-item all-item"
        class:active={!selected}
        role="option"
        tabindex="-1"
        aria-selected={!selected}
        onclick={() => select(null, "")}
        onkeydown={(e) => { if (e.key === "Enter") select(null, ""); }}
      >
        <span class="item-icon">📋</span>
        <span class="item-title">All meetings</span>
      </div>

      {#each filteredSessions() as s}
        <div
          class="session-picker-item"
          class:active={selected === s.id}
          role="option"
          tabindex="-1"
          aria-selected={selected === s.id}
          onclick={() => select(s.id, s.title || s.id.slice(0, 8))}
          onkeydown={(e) => { if (e.key === "Enter") select(s.id, s.title || s.id.slice(0, 8)); }}
        >
          <span class="item-icon">🗓️</span>
          <span class="item-main">
            <span class="item-title">{s.title || s.id.slice(0, 12)}</span>
            <span class="item-meta">
              {formatDate(s.started_at)}
              {#if s.duration_seconds}<span class="item-sep">·</span>{fmtDuration(s.duration_seconds)}{/if}
              {#if s.meeting_app}<span class="item-sep">·</span><span class="item-badge">{s.meeting_app}</span>{/if}
            </span>
          </span>
        </div>
      {/each}

      {#if filteredSessions().length === 0}
        <div class="session-picker-empty">No matching meetings</div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .session-picker {
    position: relative;
    min-width: 220px;
  }
  .session-picker-input {
    width: 100%;
    background: var(--bg-base);
    color: var(--text-primary);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 5px 28px 5px 10px;
    font-size: 12px;
    font-family: inherit;
    outline: none;
    transition: border-color 0.15s;
    box-sizing: border-box;
  }
  .session-picker-input:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px rgba(124, 108, 255, 0.15);
  }
  .session-picker-input::placeholder { color: var(--text-tertiary); }
  .session-picker-clear {
    position: absolute;
    right: 6px; top: 50%; transform: translateY(-50%);
    background: none; border: none; color: var(--text-tertiary);
    cursor: pointer; font-size: 14px; line-height: 1; padding: 2px;
  }
  .session-picker-clear:hover { color: var(--text-primary); }
  .session-picker-dropdown {
    position: absolute;
    top: calc(100% + 4px);
    left: 0; right: 0;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 8px;
    max-height: 320px;
    overflow-y: auto;
    z-index: 100;
    box-shadow: 0 8px 24px rgba(0,0,0,0.4);
  }
  .session-picker-item {
    padding: 7px 12px;
    cursor: pointer;
    display: flex;
    align-items: flex-start;
    gap: 8px;
    transition: background 0.1s;
  }
  .session-picker-item:hover { background: var(--bg-elevated); }
  .session-picker-item.active { background: rgba(124, 108, 255, 0.15); color: var(--accent); }
  .session-picker-item.all-item {
    border-bottom: 1px solid var(--border);
    font-weight: 600;
    color: var(--text-secondary);
    font-size: 12px;
    align-items: center;
  }
  .item-icon { flex-shrink: 0; font-size: 13px; margin-top: 1px; }
  .item-main { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .item-title { font-size: 12px; font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .item-meta { font-size: 10px; color: var(--text-tertiary); display: flex; align-items: center; gap: 4px; flex-wrap: wrap; }
  .item-sep { opacity: 0.5; }
  .item-badge {
    background: var(--bg-base);
    border-radius: 3px;
    padding: 0 4px;
    font-size: 9px;
    font-weight: 600;
    color: var(--accent);
  }
  .session-picker-empty {
    padding: 12px;
    text-align: center;
    color: var(--text-tertiary);
    font-size: 11px;
    font-style: italic;
  }
</style>
