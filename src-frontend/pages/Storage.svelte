<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "../lib/tauri";

  let storageInfo = $state<any>(null);
  let loading = $state(true);
  let actionMsg = $state("");

  onMount(load);

  async function load() {
    loading = true;
    try { storageInfo = await invoke("get_storage_info"); } catch (e) { console.error(e); }
    loading = false;
  }

  function fmtBytes(b: number): string {
    if (b < 1024) return `${b} B`;
    if (b < 1048576) return `${(b / 1024).toFixed(1)} KB`;
    if (b < 1073741824) return `${(b / 1048576).toFixed(1)} MB`;
    return `${(b / 1073741824).toFixed(2)} GB`;
  }

  async function deleteAudio(sessionId: string) {
    if (!confirm(`Delete audio files for ${sessionId}? Transcript will be kept.`)) return;
    try {
      const result: string = await invoke("delete_session_audio", { sessionId });
      actionMsg = result;
      setTimeout(() => actionMsg = "", 3000);
      await load();
    } catch (e) { actionMsg = `Error: ${e}`; }
  }

  async function deleteSession(sessionId: string) {
    if (!confirm(`Delete session ${sessionId} entirely? This removes audio AND transcript. Cannot be undone.`)) return;
    try {
      const result: string = await invoke("delete_full_session", { sessionId });
      actionMsg = result;
      setTimeout(() => actionMsg = "", 3000);
      await load();
    } catch (e) { actionMsg = `Error: ${e}`; }
  }
</script>

<div class="page-header">
  <h2>Storage</h2>
  <div class="header-actions">
    <button class="btn btn-xs btn-ghost" onclick={load}>Refresh</button>
    {#if actionMsg}<span style="font-size:11px;color:var(--success)">{actionMsg}</span>{/if}
  </div>
</div>

{#if loading}
  <div class="empty-state">Loading storage info...</div>
{:else if storageInfo}
  <!-- Summary cards -->
  <div class="storage-summary">
    <div class="storage-stat">
      <span class="storage-stat-value">{storageInfo.total_sessions}</span>
      <span class="storage-stat-label">Sessions</span>
    </div>
    <div class="storage-stat">
      <span class="storage-stat-value">{fmtBytes(storageInfo.total_audio_bytes)}</span>
      <span class="storage-stat-label">Audio Files</span>
    </div>
    <div class="storage-stat">
      <span class="storage-stat-value">{fmtBytes(storageInfo.database_bytes)}</span>
      <span class="storage-stat-label">Database</span>
    </div>
    <div class="storage-stat">
      <span class="storage-stat-value">{fmtBytes(storageInfo.total_bytes)}</span>
      <span class="storage-stat-label">Total</span>
    </div>
  </div>

  <!-- Session list -->
  <div class="card">
    <div class="card-header">Sessions ({storageInfo.sessions.length})</div>
    <div class="storage-list">
      {#each storageInfo.sessions as s}
        <div class="storage-row">
          <div class="storage-row-info">
            <span class="storage-row-title">{s.title || s.session_id}</span>
            <span class="storage-row-meta">
              {new Date(s.started_at).toLocaleDateString([], { month: "short", day: "numeric", year: "numeric" })}
              &middot; {s.transcript_utterances} utterances
              &middot; {s.audio_files.length} audio file{s.audio_files.length !== 1 ? "s" : ""}
            </span>
          </div>
          <div class="storage-row-size">
            {#if s.audio_total_bytes > 0}
              <span class="storage-size-badge">{fmtBytes(s.audio_total_bytes)}</span>
            {:else}
              <span class="storage-size-badge muted">No audio</span>
            {/if}
          </div>
          <div class="storage-row-actions">
            {#if s.audio_total_bytes > 0}
              <button class="btn btn-xs btn-ghost" onclick={() => deleteAudio(s.session_id)} title="Delete audio only (keep transcript)">
                Delete Audio
              </button>
            {/if}
            <button class="btn btn-xs btn-ghost btn-danger" onclick={() => deleteSession(s.session_id)} title="Delete entire session">
              Delete All
            </button>
          </div>
        </div>
      {/each}
      {#if storageInfo.sessions.length === 0}
        <div class="empty-state">No sessions stored.</div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .storage-summary {
    display: grid; grid-template-columns: repeat(4, 1fr); gap: 12px;
  }
  .storage-stat {
    background: var(--bg-secondary); border: 1px solid var(--border-subtle);
    border-radius: 10px; padding: 16px; text-align: center;
    display: flex; flex-direction: column; gap: 4px;
  }
  .storage-stat-value { font-size: 20px; font-weight: 700; color: var(--text-primary); }
  .storage-stat-label { font-size: 11px; color: var(--text-tertiary); text-transform: uppercase; letter-spacing: 0.5px; }
  .storage-list { max-height: 500px; overflow-y: auto; }
  .storage-row {
    display: flex; align-items: center; gap: 12px;
    padding: 10px 16px; border-bottom: 1px solid var(--border-subtle);
  }
  .storage-row:last-child { border-bottom: none; }
  .storage-row:hover { background: var(--bg-elevated); }
  .storage-row-info { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .storage-row-title { font-size: 13px; font-weight: 500; }
  .storage-row-meta { font-size: 11px; color: var(--text-tertiary); }
  .storage-row-size { min-width: 80px; text-align: right; }
  .storage-size-badge {
    font-size: 11px; font-weight: 600; padding: 2px 8px;
    border-radius: 4px; background: rgba(124,108,255,0.1); color: var(--accent);
  }
  .storage-size-badge.muted { background: var(--bg-base); color: var(--text-tertiary); }
  .storage-row-actions { display: flex; gap: 4px; flex-shrink: 0; }
</style>
