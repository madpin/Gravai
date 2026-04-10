<script lang="ts">
  import { onMount } from "svelte";
  import { invoke, fmtDuration } from "../lib/tauri";
  import TranscriptView from "../components/TranscriptView.svelte";
  import Icon from "../components/Icon.svelte";
  import { pendingArchiveSessionId } from "../lib/store";
  import { get } from "svelte/store";

  let sessions = $state<any[]>([]);
  let selectedId = $state<string | null>(null);
  let utterances = $state<any[]>([]);
  let summary = $state<any>(null);
  let summaryLoading = $state(false);
  let searchQuery = $state("");
  let searchMode = $state<"keyword" | "semantic" | "hybrid">("keyword");
  let searchResults = $state<any[] | null>(null);
  let exportFormats = $state<any[]>([]);
  let exportFormat = $state("m4a-aac");
  let exportMsg = $state("");

  // Filters
  let filterApp = $state("");
  let filterDateFrom = $state("");
  let filterDateTo = $state("");

  onMount(async () => {
    await load();
    loadFormats();
    const pending = get(pendingArchiveSessionId);
    if (pending) {
      pendingArchiveSessionId.set(null);
      await select(pending);
    }
  });

  async function load() {
    try {
      if (filterApp || filterDateFrom || filterDateTo) {
        sessions = await invoke("search_sessions_filtered", {
          dateFrom: filterDateFrom || null,
          dateTo: filterDateTo || null,
          meetingApp: filterApp || null,
        });
      } else {
        sessions = await invoke("list_sessions");
      }
    } catch (_) {}
  }

  async function select(id: string) {
    selectedId = id;
    summary = null;
    searchResults = null;
    try { utterances = await invoke("get_transcript", { sessionId: id }); } catch (_) {}
    // Auto-generate embeddings in background
    invoke("generate_embeddings", { sessionId: id }).catch(() => {});
  }

  async function search() {
    const q = searchQuery.trim();
    if (!q) { searchResults = null; return; }
    try {
      if (searchMode === "semantic") {
        const results: any[] = await invoke("semantic_search", { query: q, limit: 20 });
        searchResults = results.map(r => r.utterance);
      } else if (searchMode === "hybrid") {
        const results: any[] = await invoke("hybrid_search", { query: q });
        searchResults = results.map(r => r.utterance);
      } else {
        searchResults = await invoke("search_utterances", { query: q });
      }
    } catch (_) { searchResults = []; }
  }

  async function summarize() {
    if (!selectedId) return;
    summaryLoading = true;
    try { summary = await invoke("summarize_session", { sessionId: selectedId }); } catch (_) {}
    summaryLoading = false;
  }

  async function loadFormats() {
    try { exportFormats = await invoke("get_export_formats"); } catch (_) {}
  }

  async function exportAudio() {
    if (!selectedId) return;
    try {
      const path = await invoke("export_session_audio", { sessionId: selectedId, format: exportFormat });
      exportMsg = `Audio exported: ${path}`; setTimeout(() => exportMsg = "", 3000);
    } catch (e) { exportMsg = `Error: ${e}`; }
  }

  async function exportMd() {
    if (!selectedId) return;
    try {
      const md: string = await invoke("export_markdown", { sessionId: selectedId });
      const blob = new Blob([md], { type: "text/markdown" });
      const a = document.createElement("a"); a.href = URL.createObjectURL(blob);
      a.download = `${selectedId}.md`; a.click();
      exportMsg = "Markdown downloaded"; setTimeout(() => exportMsg = "", 3000);
    } catch (e) { exportMsg = `Error: ${e}`; }
  }

  let searchTimeout: number;

  let selectedSession = $derived(sessions.find(s => s.id === selectedId) ?? null);
  let editingSessionTitle = $state(false);
  let sessionTitleEdit = $state("");
  let showDeleteConfirm = $state(false);
  let deleteLoading = $state(false);

  async function deleteSession() {
    if (!selectedId) return;
    deleteLoading = true;
    try {
      await invoke("delete_full_session", { sessionId: selectedId });
      showDeleteConfirm = false;
      selectedId = null;
      utterances = [];
      summary = null;
      await load();
    } catch (e) {
      exportMsg = `Delete failed: ${e}`;
    }
    deleteLoading = false;
  }

  function startSessionTitleEdit() {
    sessionTitleEdit = selectedSession?.title ?? "";
    editingSessionTitle = true;
  }

  async function saveSessionTitle() {
    const trimmed = sessionTitleEdit.trim();
    editingSessionTitle = false;
    if (!selectedId) return;
    if (trimmed === (selectedSession?.title ?? "")) return;
    try {
      await invoke("rename_session", { sessionId: selectedId, title: trimmed });
      await load();
    } catch (_) {}
  }
</script>

<style>
  .search-row { display: flex; gap: 4px; }
  .search-input { max-width: none; flex: 1; }
  .select-mode { min-width: 90px; font-size: 11px; padding: 4px 8px; }
  .filter-summary { font-size: 11px; color: var(--text-tertiary); cursor: pointer; }
  .filter-body { display: flex; flex-direction: column; gap: 4px; padding: 6px 0; }
  .filter-input { max-width: none; font-size: 11px; }
  .select-compact { min-width: 100px; font-size: 11px; }
  .archive-detail-header {
    display: flex; align-items: center; gap: 8px;
    padding: 16px 16px 12px;
    border-bottom: 1px solid var(--border-subtle);
    margin-bottom: 8px;
  }
  .archive-session-title {
    font-size: 16px; font-weight: 600; color: var(--text-primary);
    flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .archive-title-input {
    font-size: 15px; font-weight: 600; flex: 1;
    background: var(--bg-elevated); border: 1px solid var(--accent-dim);
    border-radius: var(--radius-sm); color: var(--text-primary);
    padding: 3px 10px; font-family: inherit;
  }
  .title-edit-btn {
    background: none; border: none; cursor: pointer; color: var(--text-tertiary);
    padding: 2px 4px; border-radius: 4px;
    display: flex; align-items: center; flex-shrink: 0;
  }
  .title-edit-btn:hover { color: var(--text-secondary); background: var(--bg-secondary); }
  .delete-btn:hover { color: var(--danger) !important; }
  .delete-overlay {
    position: fixed; inset: 0; background: rgba(0,0,0,0.5);
    display: flex; align-items: center; justify-content: center; z-index: 100;
  }
  .delete-dialog {
    background: var(--bg-primary); border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md); padding: 20px; max-width: 380px; width: 90%;
    box-shadow: 0 8px 32px rgba(0,0,0,0.3);
  }
  .delete-dialog h4 { margin: 0 0 10px; font-size: 15px; color: var(--text-primary); }
  .delete-dialog p { margin: 0 0 8px; font-size: 13px; color: var(--text-secondary); line-height: 1.5; }
  .delete-warning { color: var(--danger) !important; font-weight: 500; }
  .delete-dialog-actions { display: flex; gap: 8px; justify-content: flex-end; margin-top: 16px; }
  .btn-delete-confirm {
    background: var(--danger); color: white; border: none; cursor: pointer;
    border-radius: var(--radius-sm); padding: 4px 12px; font-weight: 500; font-size: 12px;
  }
  .btn-delete-confirm:hover { opacity: 0.9; }
  .btn-delete-confirm:disabled { opacity: 0.5; cursor: not-allowed; }
</style>

<div class="page-header"><h2>Archive</h2></div>

<div class="archive-layout">
  <div class="archive-sidebar">
    <!-- Search bar -->
    <div class="search-row">
      <input class="input search-input" placeholder="Search transcripts..." bind:value={searchQuery}
        oninput={() => { clearTimeout(searchTimeout); searchTimeout = window.setTimeout(search, 400); }} />
      <select class="select select-mode" bind:value={searchMode} onchange={search}>
        <option value="keyword">Keyword</option>
        <option value="semantic">Semantic</option>
        <option value="hybrid">Hybrid</option>
      </select>
    </div>

    <!-- Filters -->
    <details class="filter-panel">
      <summary class="filter-summary">Filters</summary>
      <div class="filter-body">
        <input class="input filter-input" type="date" bind:value={filterDateFrom} onchange={load} />
        <input class="input filter-input" type="date" bind:value={filterDateTo} onchange={load} />
        <input class="input filter-input" placeholder="Meeting app..." bind:value={filterApp} onchange={load} />
      </div>
    </details>

    <!-- Session list -->
    <div class="archive-list">
      {#if sessions.length === 0}
        <div class="empty-state">No sessions yet.</div>
      {:else}
        {#each sessions as s}
          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
          <div class="archive-row" class:selected={selectedId === s.id} onclick={() => select(s.id)} role="button" tabindex="0">
            <strong>{s.title || s.id}</strong>
            <span class="archive-meta">
              {new Date(s.started_at).toLocaleDateString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })}
              &middot; {s.duration_seconds ? fmtDuration(s.duration_seconds) : "—"}
              {#if s.meeting_app}&middot; {s.meeting_app}{/if}
            </span>
          </div>
        {/each}
      {/if}
    </div>
  </div>

  <div class="archive-detail">
    {#if selectedId}
      <div class="archive-detail-header">
        {#if editingSessionTitle}
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="archive-title-input"
            bind:value={sessionTitleEdit}
            autofocus
            onblur={saveSessionTitle}
            onkeydown={(e) => { if (e.key === "Enter") saveSessionTitle(); if (e.key === "Escape") editingSessionTitle = false; }}
          />
        {:else}
          <h3 class="archive-session-title">{selectedSession?.title || selectedId}</h3>
          <button class="title-edit-btn" onclick={startSessionTitleEdit} title="Rename session" aria-label="Rename session">
            <Icon name="pencil" size={13}/>
          </button>
          <button class="title-edit-btn delete-btn" onclick={() => showDeleteConfirm = true} title="Delete session" aria-label="Delete session">
            <Icon name="trash" size={13}/>
          </button>
        {/if}
      </div>

      {#if showDeleteConfirm}
        <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
        <div class="delete-overlay" onclick={() => showDeleteConfirm = false}>
          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
          <div class="delete-dialog" onclick={(e) => e.stopPropagation()}>
            <h4>Delete Session</h4>
            <p>This will permanently delete <strong>{selectedSession?.title || selectedId}</strong>, including all audio files, transcripts, and database records.</p>
            <p class="delete-warning">This action cannot be undone.</p>
            <div class="delete-dialog-actions">
              <button class="btn btn-xs btn-ghost" onclick={() => showDeleteConfirm = false} disabled={deleteLoading}>Cancel</button>
              <button class="btn-delete-confirm" onclick={deleteSession} disabled={deleteLoading}>
                {#if deleteLoading}Deleting...{:else}Delete{/if}
              </button>
            </div>
          </div>
        </div>
      {/if}
      <div class="archive-actions">
        <button class="btn btn-xs btn-accent" onclick={summarize} disabled={summaryLoading}>
          {#if summaryLoading}Summarizing...{:else}<Icon name="file-text" size={13}/> Summarize{/if}
        </button>
        <button class="btn btn-xs btn-ghost" onclick={exportMd}><Icon name="file" size={13}/> Markdown</button>
        <select class="select select-compact" bind:value={exportFormat}>
          {#each exportFormats as f}<option value={f.id}>{f.label}</option>{/each}
        </select>
        <button class="btn btn-xs btn-ghost" onclick={exportAudio}><Icon name="speaker" size={13}/> Export Audio</button>
        {#if exportMsg}<span class="action-msg">{exportMsg}</span>{/if}
      </div>
    {/if}

    {#if summary}
      <div class="card">
        <div class="card-header">Summary</div>
        <div class="summary-content">
          <h4>TL;DR</h4><p>{summary.tldr}</p>
          {#if summary.key_decisions?.length}<h4>Key Decisions</h4><ul>{#each summary.key_decisions as d}<li>{d}</li>{/each}</ul>{/if}
          {#if summary.action_items?.length}<h4>Action Items</h4><ul>{#each summary.action_items as a}<li>{a.description} {#if a.owner}<span class="action-owner">@{a.owner}</span>{/if}</li>{/each}</ul>{/if}
          {#if summary.open_questions?.length}<h4>Open Questions</h4><ul>{#each summary.open_questions as q}<li>{q}</li>{/each}</ul>{/if}
        </div>
      </div>
    {/if}

    <TranscriptView utterances={searchResults || utterances} sessionId={selectedId} />
  </div>
</div>
