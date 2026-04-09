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

<div class="page-header">
  <div>
    <h2>Knowledge Base</h2>
    <p class="page-desc">Text blocks added here are sent to the AI to guide transcript correction — names, acronyms, project context, anything Whisper tends to get wrong.</p>
  </div>
  <button class="btn btn-accent" onclick={startNew}>+ Add Block</button>
</div>

{#if editingId !== null}
  <div class="card">
    <div class="card-header">{editingId === 0 ? "New Block" : "Edit Block"}</div>
    <div class="kb-edit-body">
      <input
        class="kb-title-input"
        type="text"
        bind:value={editTitle}
        placeholder="Title (optional)"
      />
      <textarea
        class="config-editor"
        bind:value={editContent}
        rows="5"
        placeholder="Write any context here — names and how they're spelled, project abbreviations, domain jargon, speaker roles, etc."
      ></textarea>
    </div>
    <div class="card-footer">
      <button class="btn btn-xs btn-accent" onclick={save}>Save</button>
      <button class="btn btn-xs btn-ghost" onclick={cancelEdit}>Cancel</button>
      {#if saveMsg}<span class="action-msg">{saveMsg}</span>{/if}
    </div>
  </div>
{/if}

{#if entries.length === 0}
  <div class="empty-state">
    No context blocks yet. Add blocks to help the AI fix how Whisper spelled names, terms, and jargon.
  </div>
{:else}
  <div class="kb-list">
    {#each entries as entry (entry.id)}
      <div class="kb-card">
        <div class="kb-card-header">
          <span class="kb-card-title">{entry.name}</span>
          <div class="kb-card-actions">
            <button class="btn btn-xs btn-ghost" onclick={() => startEdit(entry)} title="Edit">✎</button>
            <button class="btn btn-xs btn-ghost btn-danger" onclick={() => deleteEntry(entry.id, entry.name)} title="Delete">✕</button>
          </div>
        </div>
        {#if entry.context}
          <p class="kb-card-content">{entry.context}</p>
        {:else}
          <p class="kb-card-empty">No content.</p>
        {/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  .kb-edit-body {
    padding: 12px 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .kb-title-input {
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--border);
    padding: 4px 0;
    color: var(--text-primary);
    font-size: 14px;
    font-weight: 600;
    font-family: inherit;
    outline: none;
    width: 100%;
  }
  .kb-title-input:focus {
    border-bottom-color: var(--accent);
  }

  .kb-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .kb-card {
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    padding: 12px 16px;
    transition: border-color 0.15s var(--ease);
  }
  .kb-card:hover {
    border-color: var(--border);
  }

  .kb-card-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 6px;
  }

  .kb-card-title {
    font-weight: 600;
    font-size: 13px;
    color: var(--text-primary);
  }

  .kb-card-actions {
    display: flex;
    gap: 4px;
    opacity: 0;
    transition: opacity 0.15s;
  }
  .kb-card:hover .kb-card-actions {
    opacity: 1;
  }

  .kb-card-content {
    margin: 0;
    font-size: 12px;
    color: var(--text-secondary);
    line-height: 1.6;
    white-space: pre-wrap;
  }

  .kb-card-empty {
    margin: 0;
    font-size: 12px;
    color: var(--text-tertiary);
    font-style: italic;
  }
</style>
