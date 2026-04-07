<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke, listen } from "../lib/tauri";

  let models = $state<any[]>([]);
  let silero = $state<any>(null);
  let aiModels = $state<any[]>([]);
  let modelsDir = $state("");
  let downloading = $state<Record<string, { progress: number; status: string }>>({});
  let actionMsg = $state("");
  let unlistenDownload: (() => void) | null = null;

  onMount(async () => {
    await load();
    unlistenDownload = await listen("gravai:model-download", (e: any) => {
      const d = e.payload?.data || e.payload;
      if (!d?.model_id) return;
      downloading = { ...downloading, [d.model_id]: { progress: d.progress || 0, status: d.status || "" } };
      if (d.status === "complete" || d.status === "error") {
        setTimeout(() => {
          const { [d.model_id]: _, ...rest } = downloading;
          downloading = rest;
          load();
        }, 1500);
      }
    });
  });

  onDestroy(() => { unlistenDownload?.(); });

  async function load() {
    try {
      const info: any = await invoke("get_models_status");
      models = info.whisper_models || [];
      silero = info.silero_vad;
      aiModels = info.ai_models || [];
      modelsDir = info.models_dir || "";
    } catch (_) {}
  }

  async function download(id: string) {
    downloading = { ...downloading, [id]: { progress: 0, status: "starting" } };
    try {
      const msg: string = await invoke("download_model", { modelId: id });
      actionMsg = msg; setTimeout(() => actionMsg = "", 3000);
    } catch (e) {
      actionMsg = `Error: ${e}`; setTimeout(() => actionMsg = "", 5000);
    }
  }

  async function deleteModel(id: string) {
    if (!confirm(`Delete model ggml-${id}.bin?`)) return;
    try {
      const msg: string = await invoke("delete_model", { modelId: id });
      actionMsg = msg; setTimeout(() => actionMsg = "", 3000);
      await load();
    } catch (e) { actionMsg = `Error: ${e}`; }
  }

  function fmtBytes(b: number): string {
    if (b < 1048576) return `${(b / 1024).toFixed(0)} KB`;
    if (b < 1073741824) return `${(b / 1048576).toFixed(0)} MB`;
    return `${(b / 1073741824).toFixed(1)} GB`;
  }

  // Get current config to show which model is active
  let activeModel = $state("");
  let activeProfileName = $state("");
  onMount(async () => {
    // Read active model from config (which is set by the active profile)
    try {
      const cfg: any = await invoke("get_config");
      activeModel = cfg.transcription?.model || "medium";
    } catch (_) {}
    // Also get active profile name
    try {
      const pr: any = await invoke("get_profiles");
      if (pr.active_profile_id && pr.profiles?.[pr.active_profile_id]) {
        const p = pr.profiles[pr.active_profile_id];
        activeProfileName = p.name || pr.active_profile_id;
        // Profile's model overrides config if set
        if (p.transcription_model) activeModel = p.transcription_model;
      }
    } catch (_) {}
  });

  import { currentPage } from "../lib/store";
</script>

<div class="page-header">
  <h2>Models</h2>
  <div class="header-actions">
    {#if actionMsg}<span style="font-size:11px;color:var(--success)">{actionMsg}</span>{/if}
  </div>
</div>
<p class="page-desc">
  Manage AI models. Download the model you need before recording. Larger models are more accurate but slower.
  {#if activeProfileName}The active profile <strong>{activeProfileName}</strong> uses <strong>{activeModel}</strong>.
    <button class="btn-link" onclick={() => currentPage.set("profiles")}>Change in Profiles →</button>
  {/if}
</p>

<!-- Whisper models -->
<div class="card">
  <div class="card-header">Whisper Transcription Models</div>
  <div class="model-list">
    {#each models as m}
      <div class="model-row" class:active-model={activeModel === m.id}>
        <div class="model-info">
          <div class="model-name">
            ggml-{m.id}
            {#if activeModel === m.id}
              <span class="card-tag" style="background:var(--accent-glow);color:var(--accent)">Active{#if activeProfileName} via {activeProfileName}{/if}</span>
            {/if}
          </div>
          <div class="model-desc">
            {m.description}
            {#if activeModel === m.id && activeProfileName}
              <span style="color:var(--text-tertiary)"> — Set by profile <button class="btn-link" onclick={() => currentPage.set("profiles")}>{activeProfileName}</button></span>
            {/if}
          </div>
        </div>

        <div class="model-status">
          {#if downloading[m.id]}
            <div class="model-progress">
              <div class="model-progress-bar" style="width: {downloading[m.id].progress}%"></div>
            </div>
            <span class="model-progress-text">{downloading[m.id].progress}%</span>
          {:else if m.corrupted}
            <span class="model-size" style="color:var(--danger)">{fmtBytes(m.actual_size)} ⚠️ corrupted</span>
          {:else if m.downloaded}
            <span class="model-size">{fmtBytes(m.actual_size)}</span>
          {:else}
            <span class="model-size muted">~{fmtBytes(m.approx_size)}</span>
          {/if}
        </div>

        <div class="model-actions">
          {#if downloading[m.id]}
            <span style="font-size:11px;color:var(--text-tertiary)">Downloading...</span>
          {:else if m.corrupted}
            <span style="font-size:11px;color:var(--danger)">❌ Corrupted</span>
            <button class="btn btn-xs btn-ghost btn-danger" onclick={() => deleteModel(m.id)}>Delete</button>
            <button class="btn btn-xs btn-accent" onclick={() => { deleteModel(m.id).then(() => download(m.id)); }}>Re-download</button>
          {:else if m.downloaded}
            <span style="font-size:11px;color:var(--success)">✓ Ready</span>
            {#if activeModel !== m.id}
              <button class="btn btn-xs btn-ghost btn-danger" onclick={() => deleteModel(m.id)}>Delete</button>
            {/if}
          {:else}
            <button class="btn btn-xs btn-accent" onclick={() => download(m.id)}>Download</button>
          {/if}
        </div>
      </div>
    {/each}
  </div>
</div>

<!-- Silero VAD -->
{#if silero}
  <div class="card">
    <div class="card-header">Other Models</div>
    <div class="model-list">
      <div class="model-row">
        <div class="model-info">
          <div class="model-name">silero_vad.onnx</div>
          <div class="model-desc">{silero.description}</div>
        </div>
        <div class="model-status">
          {#if silero.downloaded}
            <span class="model-size">{fmtBytes(silero.actual_size)}</span>
          {:else}
            <span class="model-size muted">~3 MB</span>
          {/if}
        </div>
        <div class="model-actions">
          {#if silero.downloaded}
            <span style="font-size:11px;color:var(--success)">✓ Ready</span>
          {:else}
            <span style="font-size:11px;color:var(--text-tertiary)">Downloads on first use</span>
          {/if}
        </div>
      </div>
    </div>
  </div>
{/if}

<!-- AI Models: sentiment & diarization -->
{#if aiModels.length > 0}
  <div class="card">
    <div class="card-header">AI Analysis Models</div>
    <div class="model-list">
      {#each aiModels as m}
        <div class="model-row">
          <div class="model-info">
            <div class="model-name">{m.id}</div>
            <div class="model-desc">
              {m.description}
              {#if m.note}<span class="ai-model-note"> — ⚠️ {m.note}</span>{/if}
            </div>
          </div>
          <div class="model-status">
            {#if downloading[m.id]}
              <div class="model-progress">
                <div class="model-progress-bar" style="width: {downloading[m.id].progress}%"></div>
              </div>
              <span class="model-progress-text">{downloading[m.id].progress}%</span>
            {:else if m.downloaded}
              <span class="model-size">{fmtBytes(m.actual_size)}</span>
            {:else}
              <span class="model-size muted">~{fmtBytes(m.approx_size)}</span>
            {/if}
          </div>
          <div class="model-actions">
            {#if downloading[m.id]}
              <span style="font-size:11px;color:var(--text-tertiary)">Downloading...</span>
            {:else if m.downloaded}
              <span style="font-size:11px;color:var(--success)">✓ Ready</span>
              <button class="btn btn-xs btn-ghost btn-danger" onclick={() => deleteModel(m.id)}>Delete</button>
            {:else}
              <button class="btn btn-xs btn-accent" onclick={() => download(m.id)}>Download</button>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  </div>
{/if}

<p style="font-size:11px;color:var(--text-tertiary);margin-top:4px">Models stored in: {modelsDir}</p>

<style>
  .model-list { padding: 0; }
  .model-row {
    display: flex; align-items: center; gap: 16px;
    padding: 12px 16px; border-bottom: 1px solid var(--border-subtle);
    transition: background 0.1s;
  }
  .model-row:last-child { border-bottom: none; }
  .model-row:hover { background: var(--bg-elevated); }
  .model-row.active-model { background: rgba(124, 108, 255, 0.05); }
  .model-info { flex: 1; min-width: 0; }
  .model-name { font-size: 13px; font-weight: 600; font-family: "SF Mono", monospace; display: flex; align-items: center; gap: 8px; }
  .model-desc { font-size: 11px; color: var(--text-tertiary); margin-top: 2px; }
  .model-status { min-width: 90px; text-align: right; }
  .model-size { font-size: 12px; font-weight: 500; color: var(--text-secondary); }
  .model-size.muted { color: var(--text-tertiary); }
  .model-actions { min-width: 100px; display: flex; gap: 4px; justify-content: flex-end; align-items: center; }
  .model-progress {
    width: 80px; height: 6px; background: var(--bg-base); border-radius: 3px; overflow: hidden;
  }
  .model-progress-bar {
    height: 100%; background: var(--accent); border-radius: 3px;
    transition: width 0.3s;
  }
  .model-progress-text { font-size: 10px; color: var(--text-tertiary); margin-left: 4px; }
  .ai-model-note { color: var(--warning, #f59e0b); }
</style>
