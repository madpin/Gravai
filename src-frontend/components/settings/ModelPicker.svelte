<script lang="ts">
  import Icon from "../Icon.svelte";
  import { modelDownloading } from "../../lib/store";
  import { downloadModel, deleteModelFile, modelName, type ModelEntry } from "../../lib/models";
  import { fmtBytes } from "../../lib/costs";

  // Choose one model from a family. Each row shows what it costs and whether
  // the file is here; downloading and deleting happen on the same row.
  let {
    entries,
    active,
    kind = "other",
    onuse,
  }: {
    entries: ModelEntry[];
    active: string;
    kind?: "llm" | "other";
    onuse: (id: string) => void;
  } = $props();

  let error = $state("");

  async function download(id: string) {
    error = "";
    try { await downloadModel(id); } catch (e) { error = String(e); }
  }

  async function remove(entry: ModelEntry) {
    if (!confirm(`Delete ${modelName(entry.id)} from this Mac? You can download it again later.`)) return;
    try { await deleteModelFile(entry, kind); } catch (e) { error = String(e); }
  }

  function costLine(m: ModelEntry): string {
    const parts: string[] = [];
    const size = m.downloaded && m.actual_size ? m.actual_size : m.approx_size;
    if (size) parts.push(`${m.downloaded ? "" : "≈ "}${fmtBytes(size)} download`);
    if (m.ram_gb) parts.push(`≈ ${Number(m.ram_gb.toFixed(1))} GB memory`);
    return parts.join(" · ");
  }
</script>

<div class="picker" role="radiogroup">
  {#each entries as m (m.id)}
    {@const dl = $modelDownloading[m.id]}
    {@const isActive = active === m.id}
    <div class="opt" class:active={isActive} class:missing={!m.downloaded}>
      <button
        type="button"
        class="pick"
        role="radio"
        aria-checked={isActive}
        disabled={!m.downloaded}
        title={m.downloaded ? "Use this model" : "Download it first"}
        onclick={() => !isActive && onuse(m.id)}
      >
        <span class="radio" aria-hidden="true"></span>
        <span class="text">
          <span class="name">{modelName(m.id)}{#if m.speed}<span class="speed">{m.speed}</span>{/if}</span>
          <span class="meta">{costLine(m)}{#if m.note} · {m.note}{/if}</span>
        </span>
      </button>
      <span class="state">
        {#if dl}
          <span class="bar"><span style="width:{dl.progress}%"></span></span>
          <span class="pct">{dl.progress}%</span>
        {:else if m.corrupted}
          <span class="warn"><Icon name="alert-triangle" size={12}/> Damaged</span>
          <button class="btn btn-xs btn-ghost" onclick={() => remove(m).then(() => download(m.id))}>Re-download</button>
        {:else if !m.downloaded}
          <button class="btn btn-xs btn-ghost" onclick={() => download(m.id)}>Download</button>
        {:else if !isActive && m.approx_size > 0}
          <button class="icon-btn" title="Delete file" aria-label="Delete {modelName(m.id)}" onclick={() => remove(m)}><Icon name="trash" size={13}/></button>
        {/if}
      </span>
    </div>
  {/each}
  {#if error}<p class="err">{error}</p>{/if}
</div>

<style>
  .picker { display: flex; flex-direction: column; }
  .opt { display: flex; align-items: center; gap: 10px; padding: 8px 18px 8px 12px; border-bottom: 1px solid var(--border-subtle); }
  .opt:last-of-type { border-bottom: none; }
  .pick {
    flex: 1; min-width: 0; display: flex; align-items: center; gap: 10px; text-align: left;
    background: none; border: none; color: inherit; font: inherit; padding: 4px 6px; border-radius: 6px; cursor: pointer;
  }
  .pick:hover:not(:disabled) { background: var(--bg-elevated); }
  .pick:disabled { cursor: default; }
  .radio { width: 14px; height: 14px; border-radius: 50%; border: 1.5px solid var(--text-tertiary); flex-shrink: 0; position: relative; }
  .active .radio { border-color: var(--accent); }
  .active .radio::after { content: ""; position: absolute; inset: 2.5px; border-radius: 50%; background: var(--accent); }
  .text { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .name { font-size: 13px; color: var(--text-primary); display: flex; align-items: center; gap: 8px; }
  .missing .name { color: var(--text-secondary); }
  .speed { font-size: 11px; color: var(--text-tertiary); }
  .meta { font-size: 11px; color: var(--text-tertiary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .state { display: flex; align-items: center; gap: 6px; flex-shrink: 0; }
  .bar { width: 80px; height: 4px; background: var(--bg-base); border-radius: 2px; overflow: hidden; }
  .bar span { display: block; height: 100%; background: var(--accent); transition: width 0.2s; }
  .pct { font-size: 11px; color: var(--text-tertiary); min-width: 32px; text-align: right; }
  .warn { font-size: 11px; color: var(--danger); display: flex; align-items: center; gap: 4px; }
  .icon-btn { background: none; border: none; color: var(--text-tertiary); cursor: pointer; padding: 4px; border-radius: 4px; display: flex; }
  .icon-btn:hover { color: var(--danger); background: var(--bg-elevated); }
  .err { margin: 0; padding: 8px 18px; font-size: 12px; color: var(--danger); }
</style>
