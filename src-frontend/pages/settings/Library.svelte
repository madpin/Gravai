<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "../../lib/tauri";
  import { config } from "../../lib/config";
  import { models, loadModels, deleteModelFile, modelName, type ModelEntry } from "../../lib/models";
  import { fmtBytes } from "../../lib/costs";
  import { navigate } from "../../lib/store";
  import Group from "../../components/settings/Group.svelte";
  import Icon from "../../components/Icon.svelte";
  import { incomplete, scanIncomplete, recoveryOpen } from "../../lib/recovery";

  let checking = $state(false);
  async function checkUnfinished() {
    checking = true;
    const found = await scanIncomplete();
    checking = false;
    if (found.length) recoveryOpen.set(true);
  }

  // ── Knowledge ────────────────────────────────────────────────────────
  interface KnowledgeEntry {
    id: number; category: string; name: string; aliases: string | null;
    context: string | null; active: boolean; created_at: string; updated_at: string;
  }
  let entries = $state<KnowledgeEntry[]>([]);
  let editingId = $state<number | null>(null); // null = closed, 0 = new
  let editTitle = $state("");
  let editContent = $state("");
  let kbMsg = $state("");

  async function loadKnowledge() {
    try { entries = await invoke<KnowledgeEntry[]>("list_knowledge", { activeOnly: false }); } catch (_) {}
  }
  function edit(entry: KnowledgeEntry | null) {
    editingId = entry?.id ?? 0;
    editTitle = entry?.name ?? "";
    editContent = entry?.context ?? "";
    kbMsg = "";
  }
  async function saveEntry() {
    if (!editTitle.trim() && !editContent.trim()) { kbMsg = "Add a title or some text."; return; }
    try {
      await invoke("upsert_knowledge", { entry: {
        id: editingId ?? 0, category: "other", name: editTitle.trim() || "Untitled", aliases: null,
        context: editContent.trim() || null, active: true, created_at: "", updated_at: "",
      } });
      editingId = null;
      await loadKnowledge();
    } catch (e) { kbMsg = String(e); }
  }
  async function deleteEntry(entry: KnowledgeEntry) {
    if (!confirm(`Delete “${entry.name}”?`)) return;
    await invoke("delete_knowledge", { id: entry.id }).catch(() => {});
    await loadKnowledge();
  }

  // ── Recordings on disk ───────────────────────────────────────────────
  let storage = $state<any>(null);
  let showAllSessions = $state(false);
  async function loadStorage() {
    try { storage = await invoke("get_storage_info"); } catch (_) {}
  }
  let bySize = $derived(
    [...(storage?.sessions ?? [])].sort((a: any, b: any) => b.audio_total_bytes - a.audio_total_bytes),
  );
  async function deleteAudio(s: any) {
    if (!confirm(`Delete the audio of “${s.title || s.session_id}”? The transcript stays.`)) return;
    await invoke("delete_session_audio", { sessionId: s.session_id }).catch(() => {});
    await loadStorage();
  }
  async function deleteRecording(s: any) {
    if (!confirm(`Delete “${s.title || s.session_id}” completely? Audio and transcript are removed. This cannot be undone.`)) return;
    await invoke("delete_full_session", { sessionId: s.session_id }).catch(() => {});
    await loadStorage();
  }

  // ── Model files ──────────────────────────────────────────────────────
  type FileRow = { entry: ModelEntry; kind: "llm" | "other"; usedBy: string | null; section: string };
  let files = $derived.by((): FileRow[] => {
    const m = $models, c = $config;
    if (!m || !c) return [];
    const rows: FileRow[] = [];
    const add = (list: ModelEntry[], kind: "llm" | "other", section: string, activeId: string | null, use: string) => {
      for (const e of list) {
        if (!e.downloaded && !e.corrupted) continue;
        rows.push({ entry: e, kind, section, usedBy: e.id === activeId ? use : null });
      }
    };
    add(m.whisper_models, "other", "speech", c.transcription.model, "Speech");
    add(m.llm_models, "llm", "assistant", c.llm.provider === "local" ? c.llm.local_model : null, "Assistant");
    add(m.embedding_models, "other", "search", c.embedding.model, "Search");
    add(m.ai_models.filter(e => e.id === "go-emotions"), "other", "speech", c.features.sentiment.enabled ? "go-emotions" : null, "Emotion");
    add(m.ai_models.filter(e => e.id === "pyannote-segmentation"), "other", "speech", c.features.diarization.model === "pyannote" ? "pyannote-segmentation" : null, "Speaker labels");
    if (m.silero_vad) add([m.silero_vad], "other", "speech", c.vad.engine === "silero" ? "silero_vad" : null, "Voice detection");
    return rows;
  });
  let modelsTotal = $derived(files.reduce((n, f) => n + (f.entry.actual_size ?? 0), 0));

  async function deleteFile(f: FileRow) {
    if (!confirm(`Delete ${modelName(f.entry.id)}? You can download it again later.`)) return;
    await deleteModelFile(f.entry, f.kind).catch(() => {});
  }

  onMount(() => { loadKnowledge(); loadStorage(); loadModels(); });
</script>

<Group title="Knowledge" note="Names, acronyms, and project context. Transcript correction uses these to fix what Whisper gets wrong.">
  {#each entries as entry (entry.id)}
    {#if editingId === entry.id}
      {@render editor()}
    {:else}
      <div class="kb-row">
        <div class="kb-text">
          <span class="kb-name">{entry.name}</span>
          {#if entry.context}<span class="kb-ctx">{entry.context}</span>{/if}
        </div>
        <button class="icon-btn" title="Edit" aria-label="Edit {entry.name}" onclick={() => edit(entry)}><Icon name="pencil" size={13}/></button>
        <button class="icon-btn danger" title="Delete" aria-label="Delete {entry.name}" onclick={() => deleteEntry(entry)}><Icon name="trash" size={13}/></button>
      </div>
    {/if}
  {/each}
  {#if editingId === 0}
    {@render editor()}
  {:else if editingId === null}
    <div class="add-row">
      {#if !entries.length}<span class="empty">Nothing here yet.</span>{/if}
      <button class="btn btn-xs btn-ghost" onclick={() => edit(null)}><Icon name="plus" size={12}/> Add entry</button>
    </div>
  {/if}
</Group>

{#snippet editor()}
  <div class="kb-edit">
    <input class="input" bind:value={editTitle} placeholder="Name, e.g. Acme Corp" />
    <textarea class="config-editor" rows="3" bind:value={editContent} placeholder="What Whisper should know: spellings, people, project names…"></textarea>
    <div class="kb-actions">
      {#if kbMsg}<span class="kb-msg">{kbMsg}</span>{/if}
      <button class="btn btn-xs btn-ghost" onclick={() => (editingId = null)}>Cancel</button>
      <button class="btn btn-xs btn-accent" onclick={saveEntry}>Save</button>
    </div>
  </div>
{/snippet}

<Group title="Recordings on disk">
  {#if storage}
    <div class="stats">
      <div><strong>{storage.total_sessions}</strong><span>recordings</span></div>
      <div><strong>{fmtBytes(storage.total_audio_bytes)}</strong><span>audio</span></div>
      <div><strong>{fmtBytes(storage.database_bytes)}</strong><span>transcripts and search</span></div>
      <div><strong>{fmtBytes(modelsTotal)}</strong><span>models</span></div>
    </div>
    <div class="rec-row unfinished">
      <div class="rec-text">
        <span class="rec-name">Unfinished recordings</span>
        <span class="rec-meta">{$incomplete.length ? `${$incomplete.length} recording${$incomplete.length === 1 ? "" : "s"} stopped without finishing. Convert, keep, or delete them.` : "Recordings Gravai closed on before they were stopped. Checked every time Gravai opens."}</span>
      </div>
      {#if $incomplete.length}
        <button class="btn btn-xs btn-accent" onclick={() => recoveryOpen.set(true)}>Review {$incomplete.length}</button>
      {:else}
        <button class="btn btn-xs btn-ghost" disabled={checking} onclick={checkUnfinished}>{checking ? "Checking…" : "Check now"}</button>
      {/if}
    </div>
    {#each (showAllSessions ? bySize : bySize.slice(0, 5)) as s (s.session_id)}
      <div class="rec-row">
        <div class="rec-text">
          <span class="rec-name">{s.title || s.session_id}</span>
          <span class="rec-meta">{new Date(s.started_at).toLocaleDateString()} · {fmtBytes(s.audio_total_bytes)} audio · {s.transcript_utterances} sentences</span>
        </div>
        {#if s.audio_total_bytes > 0}<button class="btn btn-xs btn-ghost" onclick={() => deleteAudio(s)}>Delete audio</button>{/if}
        <button class="icon-btn danger" title="Delete recording" aria-label="Delete recording" onclick={() => deleteRecording(s)}><Icon name="trash" size={13}/></button>
      </div>
    {/each}
    {#if bySize.length > 5}
      <div class="add-row"><button class="btn btn-xs btn-ghost" onclick={() => (showAllSessions = !showAllSessions)}>{showAllSessions ? "Show largest only" : `Show all ${bySize.length}`}</button></div>
    {/if}
  {:else}
    <div class="add-row"><span class="empty">Loading…</span></div>
  {/if}
</Group>

<Group title="Model files" note="Downloaded models. Choose which one is used in the section named next to it.">
  {#each files as f (f.section + f.entry.id)}
    <div class="rec-row">
      <div class="rec-text">
        <span class="rec-name">{modelName(f.entry.id)}</span>
        <span class="rec-meta">{fmtBytes(f.entry.actual_size ?? 0)}{#if f.entry.corrupted} · damaged{/if}</span>
      </div>
      {#if f.usedBy}
        <button class="used" onclick={() => navigate(`settings:${f.section}`)}>Used by {f.usedBy}</button>
      {:else}
        <button class="icon-btn danger" title="Delete file" aria-label="Delete {modelName(f.entry.id)}" onclick={() => deleteFile(f)}><Icon name="trash" size={13}/></button>
      {/if}
    </div>
  {:else}
    <div class="add-row"><span class="empty">No models downloaded yet.</span></div>
  {/each}
  {#if $models?.models_dir}<p class="dir">{$models.models_dir}</p>{/if}
</Group>

<style>
  .kb-row, .rec-row { display: flex; align-items: center; gap: 8px; padding: 10px 18px; border-bottom: 1px solid var(--border-subtle); }
  .kb-text, .rec-text { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .kb-name, .rec-name { font-size: 13px; color: var(--text-primary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .kb-ctx, .rec-meta { font-size: 11px; color: var(--text-tertiary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .kb-edit { display: flex; flex-direction: column; gap: 8px; padding: 12px 18px; border-bottom: 1px solid var(--border-subtle); background: var(--bg-elevated); }
  .kb-edit .input { max-width: none; }
  .kb-actions { display: flex; justify-content: flex-end; align-items: center; gap: 6px; }
  .kb-msg { font-size: 12px; color: var(--danger); margin-right: auto; }
  .add-row { display: flex; align-items: center; gap: 10px; padding: 10px 18px; }
  .empty { font-size: 12px; color: var(--text-tertiary); }
  .icon-btn { background: none; border: none; color: var(--text-tertiary); cursor: pointer; padding: 4px; border-radius: 4px; display: flex; }
  .icon-btn:hover { background: var(--bg-elevated); color: var(--text-primary); }
  .icon-btn.danger:hover { color: var(--danger); }
  .stats { display: grid; grid-template-columns: repeat(4, 1fr); gap: 1px; background: var(--border-subtle); border-bottom: 1px solid var(--border-subtle); }
  .stats div { background: var(--bg-secondary); padding: 12px 18px; display: flex; flex-direction: column; gap: 2px; }
  .stats strong { font-size: 16px; color: var(--text-primary); }
  .stats span { font-size: 11px; color: var(--text-tertiary); }
  .stats div:first-child { border-top-left-radius: var(--radius-md); }
  .stats div:nth-child(4) { border-top-right-radius: var(--radius-md); }
  .used { font-size: 11px; color: var(--accent); background: var(--accent-glow); border: none; border-radius: 4px; padding: 3px 8px; cursor: pointer; }
  .dir { margin: 0; padding: 8px 18px; font-size: 11px; color: var(--text-tertiary); border-top: 1px solid var(--border-subtle); font-family: "SF Mono", monospace; }
</style>
