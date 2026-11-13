<script lang="ts">
  import { invoke } from "../lib/tauri";
  import { config, runSummary } from "../lib/config";
  import { isRecording, navigate, addAlert } from "../lib/store";
  import { downloadModel, modelName } from "../lib/models";
  import { fmtBytes } from "../lib/costs";
  import Icon from "./Icon.svelte";

  // What the next recording will use, plus saved setups and recent starts.
  // Applying is one-shot: it writes the settings and nothing stays "active".

  let open = $state(false);
  let setups = $state<any[]>([]);
  let recents = $state<any[]>([]);
  let naming = $state(false);
  let newName = $state("");
  let menuFor = $state<string | null>(null);
  let renaming = $state<string | null>(null);
  let renameValue = $state("");
  let error = $state("");

  let summary = $derived(runSummary($config));
  let currentSetup = $derived(setups.find(s => s.current));

  // Refresh when settings change elsewhere so the check mark stays right.
  $effect(() => { $config; refresh(); });

  function missingSize(item: any): number {
    return (item.missing ?? []).reduce((n: number, m: any) => n + (m.approx_size ?? 0), 0);
  }
  function missingNames(item: any): string {
    return (item.missing ?? []).map((m: any) => modelName(m.id)).join(", ");
  }

  async function refresh() {
    try { setups = await invoke("list_setups"); } catch (_) {}
    try { recents = await invoke("list_recent_starts"); } catch (_) {}
  }

  function toggle() {
    open = !open;
    naming = false; menuFor = null; renaming = null; error = "";
    if (open) refresh();
  }

  function appliedNotice(name: string, previous: any, item: any) {
    const later = $isRecording ? " Takes effect on the next recording." : "";
    const missing = item.missing ?? [];
    const actions = [{ label: "Undo", handler: () => { invoke("apply_snapshot", { snapshot: previous }); } }];
    if (missing.length) {
      actions.unshift({
        label: `Download ${fmtBytes(missingSize(item))}`,
        handler: () => { missing.forEach((m: any) => downloadModel(m.id).catch(() => {})); },
      });
    }
    addAlert({
      level: missing.length ? "warning" : "info",
      message: missing.length
        ? `Applied “${name}”. It needs ${missingNames(item)}, which ${missing.length > 1 ? "aren't" : "isn't"} downloaded yet.${later}`
        : `Applied “${name}”.${later}`,
      actions,
      dismissable: true,
    });
  }

  async function applySetup(s: any) {
    open = false;
    if (s.current) return;
    try { appliedNotice(s.name, await invoke("apply_setup", { setupId: s.id }), s); }
    catch (e) { addAlert({ level: "error", message: String(e), dismissable: true }); }
  }

  async function applyRecent(r: any) {
    open = false;
    if (r.current) return;
    try { appliedNotice(r.title || when(r.started_at), await invoke("apply_recent", { sessionId: r.session_id }), r); }
    catch (e) { addAlert({ level: "error", message: String(e), dismissable: true }); }
  }

  async function saveCurrent() {
    if (!newName.trim()) return;
    try {
      await invoke("save_current_setup", { name: newName.trim() });
      naming = false; newName = "";
      await refresh();
    } catch (e) { error = String(e); }
  }

  async function rename(s: any) {
    if (!renameValue.trim()) return;
    try { await invoke("rename_setup", { setupId: s.id, name: renameValue.trim() }); renaming = null; await refresh(); }
    catch (e) { error = String(e); }
  }

  async function remove(s: any) {
    menuFor = null;
    if (!confirm(`Delete the setup “${s.name}”? Your current settings stay as they are.`)) return;
    await invoke("delete_setup", { setupId: s.id }).catch(() => {});
    await refresh();
  }

  async function restore() {
    try { setups = await invoke("restore_builtin_setups"); } catch (_) {}
  }

  function when(iso: string): string {
    const d = new Date(iso);
    const days = Math.floor((Date.now() - d.getTime()) / 86400000);
    const time = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    if (days < 1 && d.getDate() === new Date().getDate()) return `Today ${time}`;
    if (days < 2) return `Yesterday ${time}`;
    return d.toLocaleDateString([], { weekday: "short", day: "numeric", month: "short" });
  }

  function focus(node: HTMLInputElement) { node.focus(); node.select(); }
  let hasAllBuiltins = $derived(["meeting", "podcast", "minimal"].every(id => setups.some(s => s.id === id)));
</script>

{#snippet readiness(item: any)}
  {#if item.missing?.length}
    <span class="tag needs" title="Needs {missingNames(item)}"><Icon name="download" size={10} /> {fmtBytes(missingSize(item))}</span>
  {:else}
    <span class="tag ready" title="All models are downloaded">Ready</span>
  {/if}
{/snippet}

<svelte:window onkeydown={(e) => { if (open && e.key === "Escape") open = false; }} />

<div class="runbar">
  <button class="summary" title="Change in Settings" onclick={() => navigate("settings:speech")}>
    <span>{summary}</span>
    <Icon name="chevron-right" size={12} />
  </button>
  <div class="menu-wrap">
    <button class="setups-btn" aria-haspopup="menu" aria-expanded={open} onclick={toggle} title="Saved setups and previous recordings">
      {#if currentSetup}<Icon name="check" size={12} /> {currentSetup.name}{:else}Setups{/if}
      <Icon name="chevron-down" size={12} />
    </button>
    {#if open}
      <button class="scrim" aria-label="Close" onclick={() => (open = false)}></button>
      <div class="menu" role="menu">
        <div class="menu-head">Saved setups</div>
        {#each setups as s (s.id)}
          {#if renaming === s.id}
            <div class="item editing">
              <input class="input" use:focus bind:value={renameValue} onkeydown={(e) => { if (e.key === "Enter") rename(s); if (e.key === "Escape") renaming = null; }} />
              <button class="btn btn-xs btn-accent" onclick={() => rename(s)}>Rename</button>
            </div>
          {:else}
            <div class="item" class:current={s.current}>
              <button class="item-main" role="menuitemradio" aria-checked={s.current} title={s.description || s.summary} onclick={() => applySetup(s)}>
                <span class="check" aria-hidden="true">{#if s.current}<Icon name="check" size={13} />{/if}</span>
                <span class="item-text">
                  <span class="item-name">{s.name}{#if s.current}<span class="tag in-use">In use</span>{/if}</span>
                  <span class="item-sub">{s.summary}</span>
                </span>
                {@render readiness(s)}
              </button>
              <button class="more" aria-label="Options for {s.name}" onclick={() => (menuFor = menuFor === s.id ? null : s.id)}>⋯</button>
              {#if menuFor === s.id}
                <div class="submenu">
                  <button onclick={() => { renaming = s.id; renameValue = s.name; menuFor = null; }}>Rename</button>
                  <button class="danger" onclick={() => remove(s)}>Delete</button>
                </div>
              {/if}
            </div>
          {/if}
        {:else}
          <div class="empty">No saved setups.</div>
        {/each}

        {#if naming}
          <div class="item editing">
            <input class="input" use:focus placeholder="Name, e.g. Weekly sync" bind:value={newName} onkeydown={(e) => { if (e.key === "Enter") saveCurrent(); if (e.key === "Escape") naming = false; }} />
            <button class="btn btn-xs btn-accent" disabled={!newName.trim()} onclick={saveCurrent}>Save</button>
          </div>
        {:else}
          <button class="action" onclick={() => (naming = true)}><Icon name="plus" size={12} /> Save current settings as…</button>
        {/if}

        {#if recents.length}
          <div class="menu-head">Previous recordings</div>
          {#each recents as r (r.session_id)}
            <button class="item-main recent" class:current={r.current} role="menuitemradio" aria-checked={r.current} title="Use the settings this recording started with" onclick={() => applyRecent(r)}>
              <span class="check" aria-hidden="true">{#if r.current}<Icon name="check" size={13} />{/if}</span>
              <span class="item-text">
                <span class="item-name">{r.title || "Untitled recording"} <span class="when">{when(r.started_at)}</span></span>
                <span class="item-sub">{r.summary}</span>
              </span>
              {@render readiness(r)}
            </button>
          {/each}
        {/if}

        {#if error}<div class="err">{error}</div>{/if}
        {#if !hasAllBuiltins}
          <button class="action subtle" onclick={restore}>Restore Meeting, Podcast, and Light</button>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .runbar { display: flex; align-items: flex-start; gap: 10px; min-width: 0; }
  .summary {
    display: flex; align-items: center; gap: 6px; min-width: 0; text-align: left; line-height: 1.45;
    background: none; border: none; padding: 4px 6px; margin-left: -6px; border-radius: 6px;
    font: inherit; font-size: 12px; color: var(--text-secondary); cursor: pointer;
  }
  .summary span { overflow-wrap: anywhere; }
  .summary:hover { background: var(--bg-secondary); color: var(--text-primary); }
  .menu-wrap { position: relative; margin-left: auto; flex-shrink: 0; }
  .setups-btn {
    display: flex; align-items: center; gap: 5px; max-width: 200px; white-space: nowrap; overflow: hidden; padding: 4px 10px; border-radius: 6px;
    background: var(--bg-secondary); border: 1px solid var(--border-subtle); color: var(--text-secondary);
    font: inherit; font-size: 12px; cursor: pointer;
  }
  .setups-btn:hover, .setups-btn[aria-expanded="true"] { color: var(--text-primary); border-color: var(--border); }
  .scrim { position: fixed; inset: 0; background: transparent; border: none; z-index: 40; cursor: default; }
  .menu {
    position: absolute; right: 0; top: calc(100% + 6px); z-index: 50; width: 380px; max-height: 70vh; overflow-y: auto;
    background: var(--bg-primary); border: 1px solid var(--border); border-radius: 10px;
    box-shadow: 0 12px 32px rgba(0,0,0,0.45); padding: 6px;
  }
  .menu-head { font-size: 10px; font-weight: 700; letter-spacing: 0.8px; text-transform: uppercase; color: var(--text-tertiary); padding: 8px 8px 4px; }
  .item { position: relative; display: flex; align-items: center; gap: 4px; }
  .item.editing { padding: 4px; gap: 6px; }
  .item.editing .input { flex: 1; max-width: none; }
  .item-main {
    flex: 1; min-width: 0; display: flex; align-items: center; gap: 8px; text-align: left;
    background: none; border: none; padding: 6px 8px; border-radius: 6px; color: inherit; font: inherit; cursor: pointer;
  }
  .item-main:hover { background: var(--bg-elevated); }
  .recent { width: 100%; }
  .item-text { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .check { width: 14px; flex-shrink: 0; color: var(--accent); display: flex; }
  .current .item-name { color: var(--accent); }
  .tag { flex-shrink: 0; font-size: 10px; font-weight: 600; padding: 1px 6px; border-radius: 4px; display: inline-flex; align-items: center; gap: 3px; }
  .tag.ready { color: var(--success); background: rgba(52,211,153,0.1); }
  .tag.needs { color: var(--warning); background: rgba(251,191,36,0.1); }
  .tag.in-use { color: var(--accent); background: var(--accent-glow); margin-left: 6px; }
  .item-name { font-size: 13px; color: var(--text-primary); display: flex; align-items: center; }
  .item-sub { font-size: 11px; color: var(--text-tertiary); line-height: 1.4; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  .when { font-size: 11px; color: var(--text-tertiary); margin-left: 4px; }
  .more { background: none; border: none; color: var(--text-tertiary); cursor: pointer; padding: 4px 8px; border-radius: 4px; font-size: 14px; }
  .more:hover { background: var(--bg-elevated); color: var(--text-primary); }
  .submenu {
    position: absolute; right: 4px; top: 100%; z-index: 2; display: flex; flex-direction: column; min-width: 110px;
    background: var(--bg-elevated); border: 1px solid var(--border); border-radius: 8px; padding: 4px;
  }
  .submenu button { background: none; border: none; color: var(--text-primary); font: inherit; font-size: 12px; text-align: left; padding: 6px 8px; border-radius: 4px; cursor: pointer; }
  .submenu button:hover { background: var(--bg-hover); }
  .submenu .danger { color: var(--danger); }
  .action {
    display: flex; align-items: center; gap: 6px; width: 100%; background: none; border: none; color: var(--accent);
    font: inherit; font-size: 12px; padding: 8px; border-radius: 6px; cursor: pointer; text-align: left;
  }
  .action:hover { background: var(--bg-elevated); }
  .action.subtle { color: var(--text-tertiary); border-top: 1px solid var(--border-subtle); border-radius: 0; margin-top: 4px; }
  .empty { font-size: 12px; color: var(--text-tertiary); padding: 6px 8px; }
  .err { font-size: 12px; color: var(--danger); padding: 6px 8px; }
</style>
