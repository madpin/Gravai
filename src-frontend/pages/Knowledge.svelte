<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "../lib/tauri";

  interface KnowledgeEntry {
    id: number;
    category: string;
    name: string;
    aliases: string | null;
    context: string | null;
    active: boolean;
    created_at: string;
    updated_at: string;
  }

  let entries = $state<KnowledgeEntry[]>([]);
  let editingId = $state<number | null>(null); // null = closed, 0 = new
  let editTitle = $state("");
  let editContent = $state("");
  let saveMsg = $state("");

  onMount(load);

  async function load() {
    try {
      entries = await invoke<KnowledgeEntry[]>("list_knowledge", { activeOnly: false });
    } catch (_) {}
  }

  function startNew() {
    editingId = 0;
    editTitle = "";
    editContent = "";
    saveMsg = "";
  }

  function startEdit(entry: KnowledgeEntry) {
    editingId = entry.id;
    editTitle = entry.name;
    editContent = entry.context ?? "";
    saveMsg = "";
  }

  function cancelEdit() {
    editingId = null;
    saveMsg = "";
  }

  async function save() {
    if (!editTitle.trim() && !editContent.trim()) {
      saveMsg = "Add a title or some content.";
      return;
    }

    const entry: KnowledgeEntry = {
      id: editingId ?? 0,
      category: "other",
      name: editTitle.trim() || "Untitled",
      aliases: null,
      context: editContent.trim() || null,
      active: true,
      created_at: "",
      updated_at: "",
    };

    try {
      await invoke("upsert_knowledge", { entry });
      saveMsg = "Saved.";
      setTimeout(() => { saveMsg = ""; editingId = null; }, 600);
      await load();
    } catch (err: any) {
      saveMsg = `Error: ${err}`;
    }
  }

  async function deleteEntry(id: number, name: string) {
    if (!confirm(`Delete "${name}"?`)) return;
    try {
      await invoke("delete_knowledge", { id });
      await load();
    } catch (_) {}
  }
</script>

<div class="knowledge-page">
  <div class="page-header">
    <div>
      <h2>Knowledge Base</h2>
      <p class="page-subtitle">Text blocks added here are sent to the AI to guide transcript correction — names, acronyms, project context, anything Whisper tends to get wrong.</p>
    </div>
    <button class="btn-primary" onclick={startNew}>+ Add Block</button>
  </div>

  {#if editingId !== null}
    <div class="edit-card">
      <input
        class="title-input"
        type="text"
        bind:value={editTitle}
        placeholder="Title (optional)"
      />
      <textarea
        class="content-input"
        bind:value={editContent}
        rows="5"
        placeholder="Write any context here — names and how they're spelled, project abbreviations, domain jargon, speaker roles, etc."
      ></textarea>
      <div class="form-actions">
        <button class="btn-primary" onclick={save}>Save</button>
        <button class="btn-secondary" onclick={cancelEdit}>Cancel</button>
        {#if saveMsg}<span class="save-msg">{saveMsg}</span>{/if}
      </div>
    </div>
  {/if}

  {#if entries.length === 0}
    <div class="empty-state">
      <p>No context blocks yet.</p>
      <p class="empty-hint">Add blocks to help the AI fix how Whisper spelled names, terms, and jargon.</p>
    </div>
  {:else}
    <div class="blocks-list">
      {#each entries as entry (entry.id)}
        <div class="block-card">
          <div class="block-header">
            <span class="block-title">{entry.name}</span>
            <div class="block-actions">
              <button class="btn-icon" onclick={() => startEdit(entry)} title="Edit">✎</button>
              <button class="btn-icon danger" onclick={() => deleteEntry(entry.id, entry.name)} title="Delete">✕</button>
            </div>
          </div>
          {#if entry.context}
            <p class="block-content">{entry.context}</p>
          {:else}
            <p class="block-empty">No content.</p>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .knowledge-page {
    padding: 1.5rem;
    max-width: 720px;
  }

  .page-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 1rem;
    margin-bottom: 1.5rem;
  }

  .page-header h2 {
    margin: 0 0 0.25rem;
    font-size: 1.25rem;
  }

  .page-subtitle {
    margin: 0;
    font-size: 0.85rem;
    color: var(--text-secondary, #888);
    max-width: 480px;
    line-height: 1.5;
  }

  .edit-card {
    background: var(--surface, #1e1e1e);
    border: 1px solid var(--accent, #7c6cff);
    border-radius: 8px;
    padding: 1rem;
    margin-bottom: 1.5rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }

  .title-input {
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--border, #333);
    padding: 0.3rem 0;
    color: var(--text-primary, #eee);
    font-size: 1rem;
    font-weight: 600;
    font-family: inherit;
    outline: none;
    width: 100%;
  }

  .title-input:focus {
    border-bottom-color: var(--accent, #7c6cff);
  }

  .content-input {
    background: var(--input-bg, #2a2a2a);
    border: 1px solid var(--border, #333);
    border-radius: 4px;
    padding: 0.5rem 0.7rem;
    color: var(--text-primary, #eee);
    font-size: 0.875rem;
    font-family: inherit;
    resize: vertical;
    outline: none;
    line-height: 1.6;
  }

  .content-input:focus {
    border-color: var(--accent, #7c6cff);
  }

  .form-actions {
    display: flex;
    gap: 0.75rem;
    align-items: center;
  }

  .save-msg {
    font-size: 0.85rem;
    color: var(--success, #34d399);
  }

  .empty-state {
    text-align: center;
    padding: 3rem 1rem;
    color: var(--text-secondary, #888);
    font-size: 0.9rem;
  }

  .empty-hint {
    margin-top: 0.5rem;
    font-size: 0.8rem;
    color: var(--text-tertiary, #555);
  }

  .blocks-list {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .block-card {
    background: var(--surface, #1e1e1e);
    border: 1px solid var(--border, #333);
    border-radius: 8px;
    padding: 0.9rem 1rem;
  }

  .block-card:hover {
    border-color: var(--border-hover, #444);
  }

  .block-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 0.4rem;
  }

  .block-title {
    font-weight: 600;
    font-size: 0.9rem;
    color: var(--text-primary, #eee);
  }

  .block-actions {
    display: flex;
    gap: 0.4rem;
    opacity: 0;
    transition: opacity 0.15s;
  }

  .block-card:hover .block-actions {
    opacity: 1;
  }

  .block-content {
    margin: 0;
    font-size: 0.825rem;
    color: var(--text-secondary, #999);
    line-height: 1.6;
    white-space: pre-wrap;
  }

  .block-empty {
    margin: 0;
    font-size: 0.8rem;
    color: var(--text-tertiary, #555);
    font-style: italic;
  }

  .btn-primary {
    background: var(--accent, #7c6cff);
    color: #fff;
    border: none;
    border-radius: 6px;
    padding: 0.45rem 1rem;
    cursor: pointer;
    font-size: 0.875rem;
    font-weight: 500;
    white-space: nowrap;
  }

  .btn-primary:hover {
    filter: brightness(1.1);
  }

  .btn-secondary {
    background: transparent;
    color: var(--text-secondary, #888);
    border: 1px solid var(--border, #333);
    border-radius: 6px;
    padding: 0.45rem 1rem;
    cursor: pointer;
    font-size: 0.875rem;
  }

  .btn-secondary:hover {
    color: var(--text-primary, #eee);
    border-color: var(--text-secondary, #888);
  }

  .btn-icon {
    background: transparent;
    border: 1px solid var(--border, #333);
    border-radius: 4px;
    padding: 0.2rem 0.4rem;
    cursor: pointer;
    color: var(--text-secondary, #888);
    font-size: 0.8rem;
    line-height: 1;
  }

  .btn-icon:hover {
    color: var(--text-primary, #eee);
    border-color: var(--text-secondary, #888);
  }

  .btn-icon.danger:hover {
    color: var(--danger, #f87171);
    border-color: var(--danger, #f87171);
  }
</style>
