<script lang="ts">
  import { onMount } from "svelte";
  import { invoke, fmtDuration } from "../lib/tauri";
  import TranscriptView from "../components/TranscriptView.svelte";

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

  onMount(() => { load(); loadFormats(); });

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
</script>

<style>
  .search-row { display: flex; gap: 4px; }
  .search-input { max-width: none; flex: 1; }
  .select-mode { min-width: 90px; font-size: 11px; padding: 4px 8px; }
  .filter-summary { font-size: 11px; color: var(--text-tertiary); cursor: pointer; }
  .filter-body { display: flex; flex-direction: column; gap: 4px; padding: 6px 0; }
  .filter-input { max-width: none; font-size: 11px; }
  .select-compact { min-width: 100px; font-size: 11px; }
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
      <div class="archive-actions">
        <button class="btn btn-xs btn-accent" onclick={summarize} disabled={summaryLoading}>
          {summaryLoading ? "Summarizing..." : "📝 Summarize"}
        </button>
        <button class="btn btn-xs btn-ghost" onclick={exportMd}>📄 Markdown</button>
        <select class="select select-compact" bind:value={exportFormat}>
          {#each exportFormats as f}<option value={f.id}>{f.label}</option>{/each}
        </select>
        <button class="btn btn-xs btn-ghost" onclick={exportAudio}>🔊 Export Audio</button>
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
