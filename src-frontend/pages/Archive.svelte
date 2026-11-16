<script lang="ts">
  import { onMount } from "svelte";
  import { invoke, fmtDuration, copyToClipboard } from "../lib/tauri";
  import TranscriptView from "../components/TranscriptView.svelte";
  import AudioPlayer from "../components/AudioPlayer.svelte";
  import Icon from "../components/Icon.svelte";
  import LlmStatusBanner from "../components/LlmStatusBanner.svelte";
  import { pendingArchiveSessionId, isRecording, isPaused, currentSessionId, sessionStartTime, lastSessionId, liveUtterances, liveSummary, bookmarkCount, currentPage, clearAlerts } from "../lib/store";
  import { get } from "svelte/store";

  let sessions = $state<any[]>([]);
  let selectedId = $state<string | null>(null);
  let utterances = $state<any[]>([]);
  let summary = $state<any>(null);
  let summaryLoading = $state(false);
  let summaryError = $state<string | null>(null);
  let copiedMd = $state(false);
  let copiedPath = $state(false);
  let searchQuery = $state("");
  let searchMode = $state<"keyword" | "semantic" | "hybrid">("keyword");
  let searchResults = $state<any[] | null>(null);
  let exportFormats = $state<any[]>([]);
  let exportFormat = $state("m4a-aac");
  let exportMsg = $state("");
  let bookmarks = $state<any[]>([]);

  let continuing = $state(false);
  let continueError = $state<string | null>(null);

  async function continueRecording() {
    if (!selectedId || continuing || $isRecording) return;
    continuing = true;
    continueError = null;
    const previousTranscript = utterances;
    const previousBookmarkCount = bookmarks.length;
    try {
      const result: { id: string; duration_seconds: number } = await invoke("continue_session", { sessionId: selectedId });
      currentSessionId.set(result.id);
      lastSessionId.set(result.id);
      sessionStartTime.set(Date.now() - result.duration_seconds * 1000);
      liveUtterances.set(previousTranscript);
      liveSummary.set(null);
      bookmarkCount.set(previousBookmarkCount);
      isRecording.set(true);
      isPaused.set(false);
      clearAlerts();
      currentPage.set("recording");
    } catch (e) { continueError = String(e); }
    finally { continuing = false; }
  }

  // Audio player state
  let audioPath = $state<string | null>(null);
  let audioLoading = $state(false);
  let archival = $state<any>(null);
  let audioError = $state<string | null>(null);
  let currentTimeMs = $state(-1);
  let followPlayhead = $state(false);
  let seekRequest = $state<{ ms: number; autoplay?: boolean } | null>(null);

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

  let selectionGeneration = 0;
  async function select(id: string) {
    const generation = ++selectionGeneration;
    const current = () => generation === selectionGeneration;
    selectedId = id;
    archival = null;
    continueError = null;
    summary = null;
    summaryError = null;
    searchResults = null;
    bookmarks = [];
    audioPath = null;
    audioError = null;
    currentTimeMs = -1;
    seekRequest = null;
    utterances = [];
    audioLoading = true;
    try {
      const rows = await invoke("get_transcript", { sessionId: id });
      if (!current()) return;
      utterances = rows;
    } catch (_) { if (!current()) return; }
    try {
      const rows = await invoke("list_bookmarks", { sessionId: id });
      if (!current()) return;
      bookmarks = rows;
    } catch (_) { if (!current()) return; }
    // Load any previously saved summary so the user doesn't have to re-run the LLM
    try {
      const saved = await invoke("get_session_summary", { sessionId: id });
      if (!current()) return;
      if (saved) summary = saved;
    } catch (_) { if (!current()) return; }
    try {
      const status = await invoke("get_audio_archival", { sessionId: id });
      if (!current()) return;
      archival = status;
    } catch (_) { if (!current()) return; }
    // Load audio (may trigger a merge of multiple tracks — show loading state)
    audioLoading = true;
    try {
      const path = await invoke("get_session_audio_path", { sessionId: id });
      if (!current()) return;
      audioPath = path;
    } catch (e: any) {
      if (!current()) return;
      audioError = typeof e === "string" ? e : e?.message ?? "Audio not available";
    }
    audioLoading = false;
    // Review starts from what was flagged: position the playhead at the
    // first bookmark and scroll the transcript to it — but don't autoplay.
    // With no bookmarks, behave as today (nothing seeked).
    if (audioPath && bookmarks.length > 0) {
      const firstMs = bookmarks[0].offset_ms;
      seekRequest = { ms: firstMs, autoplay: false };
      currentTimeMs = firstMs;
    }
    // Embeddings are optional. A neural model makes this the slow part of opening a session.
    try {
      const cfg: any = await invoke("get_config");
      if (cfg?.embedding?.auto_index !== false) {
        invoke("generate_embeddings", { sessionId: id }).catch(() => {});
      }
    } catch (_) {}
  }

  function fmtOffsetMs(ms: number): string {
    const secs = Math.floor(ms / 1000);
    const m = String(Math.floor(secs / 60)).padStart(2, "0");
    const s = String(secs % 60).padStart(2, "0");
    return `${m}:${s}`;
  }

  async function deleteBookmark(id: number) {
    try {
      await invoke("delete_bookmark", { bookmarkId: id });
      bookmarks = bookmarks.filter(b => b.id !== id);
    } catch (e) { exportMsg = `Error: ${e}`; }
  }

  let editingBookmarkId = $state<number | null>(null);
  let editingBookmarkValue = $state("");

  function startBookmarkNoteEdit(b: any) {
    editingBookmarkId = b.id;
    editingBookmarkValue = b.note ?? "";
  }

  async function saveBookmarkNote(id: number) {
    const trimmed = editingBookmarkValue.trim();
    editingBookmarkId = null;
    try {
      await invoke("update_bookmark_note", { bookmarkId: id, note: trimmed || null });
      bookmarks = bookmarks.map(b => b.id === id ? { ...b, note: trimmed || null } : b);
    } catch (e) { exportMsg = `Error: ${e}`; }
  }

  let rebuildingAudio = $state(false);

  /** Deletes cached playback/mix files and rebuilds from the raw source
   * tracks — the "Rebuild audio" action next to a playback error. */
  async function rebuildAudio() {
    if (!selectedId || rebuildingAudio) return;
    rebuildingAudio = true;
    try {
      audioPath = await invoke("rebuild_session_audio", { sessionId: selectedId });
      audioError = null;
    } catch (e: any) {
      audioError = typeof e === "string" ? e : e?.message ?? "Rebuild failed";
    }
    rebuildingAudio = false;
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
    summaryError = null;
    try {
      summary = await invoke("summarize_session", { sessionId: selectedId });
    } catch (e: any) {
      summaryError = typeof e === "string" ? e : (e?.message ?? "Summary failed");
    }
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

  async function copyMarkdown() {
    if (!selectedId) return;
    try {
      const md: string = await invoke("export_markdown", { sessionId: selectedId });
      await copyToClipboard(md);
      copiedMd = true;
      setTimeout(() => { copiedMd = false; }, 1500);
    } catch (e) {
      exportMsg = `Copy failed: ${e}`;
      setTimeout(() => exportMsg = "", 3000);
    }
  }

  async function copyMarkdownPath() {
    if (!selectedId) return;
    try {
      const path: string = await invoke("get_session_markdown_path", { sessionId: selectedId });
      await copyToClipboard(path);
      copiedPath = true;
      setTimeout(() => { copiedPath = false; }, 1500);
    } catch (e) {
      exportMsg = `Copy path failed: ${e}`;
      setTimeout(() => exportMsg = "", 3000);
    }
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
  .bookmarks-list { padding: 4px 0; display: flex; flex-direction: column; }
  .bookmark-row {
    display: flex; align-items: center; gap: 8px; font-size: 12px;
    padding: 5px 14px; transition: background 0.1s;
  }
  .bookmark-row:hover { background: var(--bg-elevated); }
  .bookmark-seekable { cursor: pointer; }
  .bookmark-seekable:hover { background: rgba(124,108,255,0.08); }
  .bookmark-time {
    font-family: "SF Mono", monospace; font-size: 11px; font-weight: 600;
    color: var(--accent); white-space: nowrap; min-width: 42px;
  }
  .bookmark-note { color: var(--text-secondary); flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bookmark-note-input {
    flex: 1; font-size: 12px; background: var(--bg-elevated);
    border: 1px solid var(--accent-dim); border-radius: var(--radius-sm);
    color: var(--text-primary); padding: 2px 6px; font-family: inherit;
  }
  .btn-bookmark-edit, .btn-bookmark-delete {
    background: none; border: none; cursor: pointer; color: var(--text-tertiary);
    padding: 2px; border-radius: 4px; display: flex; align-items: center; opacity: 0;
    transition: opacity 0.15s; flex-shrink: 0;
  }
  .bookmark-row:hover .btn-bookmark-edit, .bookmark-row:hover .btn-bookmark-delete { opacity: 1; }
  .btn-bookmark-edit:hover { color: var(--text-primary); }
  .btn-bookmark-delete:hover { color: var(--danger); }
  .audio-status {
    display: flex; align-items: center; gap: 6px;
    padding: 6px 16px; font-size: 11px; color: var(--text-tertiary);
    background: var(--bg-elevated); border-bottom: 1px solid var(--border-subtle);
  }
  .audio-status-warn { color: var(--warning); }
  .audio-status-msg { flex: 1; }
  .btn-rebuild {
    display: flex; align-items: center; gap: 4px; flex-shrink: 0;
    background: none; border: 1px solid var(--border-subtle); border-radius: var(--radius-sm);
    color: var(--text-secondary); font-size: 11px; cursor: pointer; padding: 2px 8px;
  }
  .btn-rebuild:hover:not(:disabled) { color: var(--text-primary); background: var(--bg-secondary); }
  .btn-rebuild:disabled { opacity: 0.6; cursor: not-allowed; }
  .archive-llm-status { padding: 4px 16px 0; }
  .archive-llm-status:empty { display: none; padding: 0; }
  .summary-error-card {
    border-left: 3px solid var(--danger);
    background: color-mix(in srgb, var(--danger) 5%, var(--bg-primary));
  }
  .summary-error-card .card-header { color: var(--danger); justify-content: space-between; }
  .summary-error-body { padding: 8px 16px 12px; }
  .summary-error-body p { margin: 0 0 6px; font-size: 12px; color: var(--text-secondary); line-height: 1.5; }
  .summary-error-hint { font-size: 11px; color: var(--text-tertiary); }
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
          <button class="title-edit-btn delete-btn" onclick={() => showDeleteConfirm = true} disabled={continuing} title="Delete session" aria-label="Delete session">
            <Icon name="trash" size={13}/>
          </button>
        {/if}
      </div>

      {#if showDeleteConfirm}
        <div
          class="delete-overlay"
          role="button"
          tabindex="0"
          onclick={() => showDeleteConfirm = false}
          onkeydown={(e) => e.key === 'Escape' && (showDeleteConfirm = false)}
        >
          <div class="delete-dialog" role="dialog" aria-modal="true" aria-label="Delete Session" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={(e) => { if (e.key !== 'Escape') e.stopPropagation(); }}>
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
        <button class="btn btn-xs btn-accent" onclick={continueRecording}
          disabled={continuing || $isRecording || selectedSession?.state !== "stopped"}
          title={$isRecording ? "Stop the active recording first" : "Add more audio to this recording"}>
          <Icon name={continuing ? "spinner" : "record"} size={13}/> {continuing ? "Continuing..." : "Continue Recording"}
        </button>
        <button class="btn btn-xs btn-accent" onclick={summarize} disabled={summaryLoading}>
          {#if summaryLoading}<Icon name="spinner" size={13}/> Generating...{:else}<Icon name="file-text" size={13}/> Generate Summary{/if}
        </button>
        <button class="btn btn-xs btn-ghost" onclick={copyMarkdown} title="Copy transcript as Markdown to clipboard">
          <Icon name={copiedMd ? "check" : "clipboard"} size={13}/> {copiedMd ? "Copied" : "Copy Markdown"}
        </button>
        <button class="btn btn-xs btn-ghost" onclick={copyMarkdownPath} title="Copy the path of the transcript's Markdown file to clipboard">
          <Icon name={copiedPath ? "check" : "file"} size={13}/> {copiedPath ? "Copied path" : "Copy path"}
        </button>
        <button class="btn btn-xs btn-ghost" onclick={exportMd} title="Download transcript as Markdown file">
          <Icon name="file" size={13}/> Markdown
        </button>
        <select class="select select-compact" bind:value={exportFormat}>
          {#each exportFormats as f}<option value={f.id}>{f.label}</option>{/each}
        </select>
        <button class="btn btn-xs btn-ghost" onclick={exportAudio}><Icon name="speaker" size={13}/> Export Audio</button>
        {#if exportMsg}<span class="action-msg">{exportMsg}</span>{/if}
      </div>

      {#if continueError}<p class="action-msg" role="alert">Could not continue recording: {continueError}</p>{/if}

      <!-- LLM loading progress: surfaces first-run download/quantize so the
           Generate Summary button doesn't look stuck. -->
      <div class="archive-llm-status">
        <LlmStatusBanner />
      </div>
    {/if}

    {#if bookmarks.length > 0}
      <div class="card">
        <div class="card-header">
          <Icon name="bookmark" size={13}/> Bookmarks ({bookmarks.length})
        </div>
        <div class="bookmarks-list">
          {#each bookmarks as b}
            {#if editingBookmarkId === b.id}
              <div class="bookmark-row">
                <span class="bookmark-time">[{fmtOffsetMs(b.offset_ms)}]</span>
                <!-- svelte-ignore a11y_autofocus -->
                <input
                  maxlength={500}
                  class="bookmark-note-input"
                  bind:value={editingBookmarkValue}
                  autofocus
                  onkeydown={(e) => { if (e.key === "Enter") saveBookmarkNote(b.id); if (e.key === "Escape") editingBookmarkId = null; }}
                  onblur={() => saveBookmarkNote(b.id)}
                />
              </div>
            {:else}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <div
                class="bookmark-row"
                class:bookmark-seekable={!!audioPath}
                role="button"
                tabindex={audioPath ? 0 : -1}
                aria-disabled={!audioPath}
                onclick={audioPath ? () => { seekRequest = { ms: b.offset_ms }; } : undefined}
                onkeydown={audioPath ? (e) => { if (e.key === 'Enter' || e.key === ' ') seekRequest = { ms: b.offset_ms }; } : undefined}
              >
                <span class="bookmark-time">[{fmtOffsetMs(b.offset_ms)}]</span>
                <span class="bookmark-note">{b.note || "—"}</span>
                <button class="btn-bookmark-edit" onclick={(e) => { e.stopPropagation(); startBookmarkNoteEdit(b); }} title="Edit note" aria-label="Edit note"><Icon name="pencil" size={11}/></button>
                <button class="btn-bookmark-delete" onclick={(e) => { e.stopPropagation(); deleteBookmark(b.id); }} title="Delete bookmark" aria-label="Delete bookmark"><Icon name="trash" size={11}/></button>
              </div>
            {/if}
          {/each}
        </div>
      </div>
    {/if}

    {#if summaryError}
      <div class="card summary-error-card">
        <div class="card-header">
          <Icon name="alert-triangle" size={13}/> Summary failed
          <button class="btn btn-xs btn-ghost" onclick={() => summaryError = null} title="Dismiss" aria-label="Dismiss">
            <Icon name="x-circle" size={12}/>
          </button>
        </div>
        <div class="summary-error-body">
          <p>{summaryError}</p>
          <p class="summary-error-hint">
            Check the LLM provider in Settings — for local models make sure the model is downloaded;
            for API providers verify the base URL and API key.
          </p>
        </div>
      </div>
    {/if}

    {#if summary}
      <div class="card">
        <div class="card-header">Summary</div>
        <div class="summary-content">
          {#if summary.tldr}
            <h4>TL;DR</h4><p>{summary.tldr}</p>
          {/if}
          {#if summary.key_decisions?.length}<h4>Key Decisions</h4><ul>{#each summary.key_decisions as d}<li>{d}</li>{/each}</ul>{/if}
          {#if summary.action_items?.length}<h4>Action Items</h4><ul>{#each summary.action_items as a}<li>{a.description} {#if a.owner}<span class="action-owner">@{a.owner}</span>{/if}</li>{/each}</ul>{/if}
          {#if summary.open_questions?.length}<h4>Open Questions</h4><ul>{#each summary.open_questions as q}<li>{q}</li>{/each}</ul>{/if}
          {#if !summary.tldr && !summary.key_decisions?.length && !summary.action_items?.length && !summary.open_questions?.length}
            <p class="empty-state">The model returned an empty summary. Try regenerating, or switch to a larger model in Settings.</p>
          {/if}
        </div>
      </div>
    {/if}

    {#if audioLoading}
      <div class="audio-status"><Icon name="spinner" size={14}/> Preparing audio (merging tracks)...</div>
    {:else if audioError}
      <div class="audio-status audio-status-warn">
        <Icon name="speaker" size={14}/> <span class="audio-status-msg">{audioError}</span>
        <button class="btn-rebuild" onclick={rebuildAudio} disabled={rebuildingAudio}>
          {#if rebuildingAudio}<Icon name="spinner" size={12}/> Rebuilding...{:else}<Icon name="refresh" size={12}/> Rebuild audio{/if}
        </button>
      </div>
    {/if}
    {#if audioPath?.endsWith(".m4a") || archival}<p style="padding: 4px 16px; font-size: 12px" role="status">{audioPath?.endsWith(".m4a") ? "Compressed copy available" : !audioPath && archival?.state === "audio_unavailable" ? "Audio no longer available · transcript retained" : ""}{#if archival?.error} Retention: {archival.error}{/if}</p>{/if}
    <AudioPlayer
      {audioPath}
      {bookmarks}
      sessionId={selectedId}
      onTimeUpdate={(ms) => currentTimeMs = ms}
      onSeekRequest={seekRequest}
      onRebuilt={(path) => { audioPath = path; audioError = null; }}
    />

    <label style="padding: 8px 16px; font-size: 12px"><input type="checkbox" bind:checked={followPlayhead} disabled={!audioPath} /> Follow audio playhead</label>
    <TranscriptView
      reviewMode={true}
      autoScroll={false}
      {followPlayhead}
      utterances={searchResults || utterances}
      sessionId={selectedId}
      sessionStartedAt={selectedSession?.started_at ?? ""}
      {bookmarks}
      {currentTimeMs}
      onUtteranceClick={audioPath ? (ms) => { seekRequest = { ms }; } : undefined}
      onBookmarkClick={audioPath ? (ms) => { seekRequest = { ms }; } : undefined}
    />
  </div>
</div>
