<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "../lib/tauri";
  import { healthStatus, currentPage } from "../lib/store";
  import Onboarding from "../components/Onboarding.svelte";

  let healthChecks = $state<any[]>([]);
  let rawJson = $state("");
  let saveMsg = $state("");
  let showWizard = $state(false);

  function runWizard() { showWizard = true; }
  let perfInfo = $state<any>(null);

  // Updates state
  let currentVersion = $state("");
  let autoCheck = $state(true);
  let checking = $state(false);
  let installing = $state(false);
  let updateInfo = $state<any>(null);
  let unlistenUpdate: (() => void) | null = null;

  onMount(async () => {
    await loadHealth();
    await loadRawConfig();
    await loadPerf();
    await loadUpdatesConfig();
    try { currentVersion = await invoke("get_app_version"); } catch (_) {}
    // Listen for auto-check result fired at startup
    const { listen } = await import("../lib/tauri");
    unlistenUpdate = await listen("gravai:update-available", (e: any) => {
      updateInfo = { ...e.payload, available: true };
    });
  });

  onDestroy(() => { unlistenUpdate?.(); });

  async function loadHealth() {
    try {
      const report: any = await invoke("get_health_report");
      healthChecks = report.checks || [];
      healthStatus.set(report.overall);
    } catch (_) {}
  }

  async function loadPerf() {
    try { perfInfo = await invoke("get_perf_snapshot"); } catch (_) {}
  }

  async function loadRawConfig() {
    try {
      const c = await invoke("get_config");
      rawJson = JSON.stringify(c, null, 2);
    } catch (_) {}
  }

  async function exportConfig() {
    const json: string = await invoke("export_config");
    const blob = new Blob([json], { type: "application/json" });
    const a = document.createElement("a"); a.href = URL.createObjectURL(blob); a.download = "gravai-config.json"; a.click();
  }

  async function importConfig() {
    const input = document.createElement("input"); input.type = "file"; input.accept = ".json";
    input.onchange = async () => {
      if (!input.files?.length) return;
      const t = await input.files[0].text();
      try { await invoke("import_config", { json: t }); await loadRawConfig(); saveMsg = "Imported!"; setTimeout(() => saveMsg = "", 1500); } catch (e: any) { saveMsg = `Error: ${e}`; }
    };
    input.click();
  }

  async function saveRawJson() {
    try { await invoke("import_config", { json: rawJson }); await loadRawConfig(); saveMsg = "JSON saved!"; setTimeout(() => saveMsg = "", 1500); } catch (e: any) { saveMsg = `Error: ${e}`; }
  }

  function goTo(page: string) { currentPage.set(page); }

  async function loadUpdatesConfig() {
    try {
      const cfg: any = await invoke("get_config");
      autoCheck = cfg.updates?.auto_check ?? true;
    } catch (_) {}
  }

  async function saveAutoCheck() {
    try { await invoke("update_config", { patch: { updates: { auto_check: autoCheck } } }); } catch (_) {}
  }

  async function checkUpdate() {
    checking = true;
    updateInfo = null;
    try { updateInfo = await invoke("check_for_update"); } catch (e: any) { updateInfo = { error: String(e) }; }
    checking = false;
  }

  async function doInstall() {
    installing = true;
    try { await invoke("install_update"); } catch (e: any) { saveMsg = `Update failed: ${e}`; setTimeout(() => saveMsg = "", 4000); installing = false; }
  }
</script>

{#if showWizard}
  <Onboarding onComplete={() => showWizard = false} />
{/if}

<div class="page-header">
  <h2>Settings</h2>
  <div class="header-actions">
    {#if saveMsg}<span style="font-size:11px;color:var(--success)">{saveMsg}</span>{/if}
    <button class="btn btn-xs btn-ghost" onclick={importConfig}>Import</button>
    <button class="btn btn-xs btn-ghost" onclick={exportConfig}>Export</button>
    <button class="btn btn-xs btn-ghost" onclick={runWizard}>🧙 Setup Wizard</button>
  </div>
</div>

<!-- Quick links to configuration pages -->
<div class="settings-links">
  <button class="settings-link-card" onclick={() => goTo("presets")}>
    <span class="settings-link-icon">🎛️</span>
    <div class="settings-link-info">
      <strong>Capture Presets</strong>
      <span>Audio sources, recording quality, format, output folder</span>
    </div>
    <span class="settings-link-arrow">→</span>
  </button>
  <button class="settings-link-card" onclick={() => goTo("profiles")}>
    <span class="settings-link-icon">👤</span>
    <div class="settings-link-info">
      <strong>Profiles</strong>
      <span>Transcription, VAD, features, AI/LLM, export settings</span>
    </div>
    <span class="settings-link-arrow">→</span>
  </button>
  <button class="settings-link-card" onclick={() => goTo("shortcuts")}>
    <span class="settings-link-icon">⌨️</span>
    <div class="settings-link-info">
      <strong>Keyboard Shortcuts</strong>
      <span>Global and local key bindings</span>
    </div>
    <span class="settings-link-arrow">→</span>
  </button>
  <button class="settings-link-card" onclick={() => goTo("storage")}>
    <span class="settings-link-icon">💿</span>
    <div class="settings-link-info">
      <strong>Storage</strong>
      <span>Manage sessions, audio files, disk usage</span>
    </div>
    <span class="settings-link-arrow">→</span>
  </button>
</div>

<!-- Health -->
<div class="card">
  <div class="card-header">System Health</div>
  <div class="health-grid">
    {#each healthChecks as check}
      <div class="health-item">
        <div class="health-item-header">
          <div class="dot {check.status}"></div>
          <span class="label">{check.name}</span>
        </div>
        <span class="msg">{check.message}</span>
      </div>
    {/each}
    {#if perfInfo}
      <div class="health-item">
        <div class="health-item-header">
          <div class="dot ok"></div>
          <span class="label">Memory</span>
        </div>
        <span class="msg">{perfInfo.rss_mb.toFixed(0)} MB / {perfInfo.total_memory_gb.toFixed(0)} GB ({perfInfo.memory_pct.toFixed(1)}%)</span>
      </div>
    {/if}
  </div>
</div>

<!-- Updates -->
<div class="card">
  <div class="card-header">
    App Updates
    {#if currentVersion}<span class="update-version-badge">v{currentVersion}</span>{/if}
  </div>

  <div class="settings-grid">
    <div class="setting-row">
      <label class="toggle-label update-toggle-label" for="auto-check-update">
        <input class="toggle" type="checkbox" id="auto-check-update" bind:checked={autoCheck} onchange={saveAutoCheck} />
        <div class="setting-info">
          <span class="setting-label">Auto-check on launch</span>
          <span class="setting-desc">Automatically check for updates when the app starts</span>
        </div>
      </label>
      <button class="btn btn-xs btn-ghost" onclick={checkUpdate} disabled={checking}>
        {checking ? "Checking…" : "Check Now"}
      </button>
    </div>
  </div>

  {#if updateInfo?.available}
    <div class="update-status-area">
      <div class="banner banner-accent">
        <div class="banner-text">
          <strong class="update-banner-title">v{updateInfo.version} available</strong>
          {#if updateInfo.body}<p class="update-notes">{updateInfo.body}</p>{/if}
        </div>
        <div class="banner-actions">
          <button class="btn btn-xs btn-accent" onclick={doInstall} disabled={installing}>
            {installing ? "Installing…" : "Download & Install"}
          </button>
        </div>
      </div>
    </div>
  {:else if updateInfo?.error}
    <div class="update-status-area">
      <div class="update-status-line update-status-error">
        <span class="update-dot update-dot-error"></span>
        Check failed — {updateInfo.error}
      </div>
    </div>
  {:else if updateInfo}
    <div class="update-status-area">
      <div class="update-status-line update-status-ok">
        <span class="update-dot update-dot-ok"></span>
        You're up to date
      </div>
    </div>
  {/if}
</div>

<!-- Advanced JSON -->
<details class="card collapsible">
  <summary class="card-header">Advanced: Raw Config JSON</summary>
  <textarea class="config-editor" rows="16" spellcheck="false" bind:value={rawJson}></textarea>
  <div class="card-footer"><button class="btn btn-xs btn-accent" onclick={saveRawJson}>Save JSON</button></div>
</details>

<style>
  .settings-links { display: grid; grid-template-columns: repeat(2, 1fr); gap: 10px; }
  .settings-link-card {
    display: flex; align-items: center; gap: 14px;
    padding: 14px 18px; background: var(--bg-secondary);
    border: 1px solid var(--border-subtle); border-radius: 10px;
    cursor: pointer; transition: all 0.15s; text-align: left;
    font-family: inherit; color: inherit;
  }
  .settings-link-card:hover { background: var(--bg-elevated); border-color: var(--border); }
  .settings-link-icon { font-size: 24px; flex-shrink: 0; }
  .settings-link-info { flex: 1; display: flex; flex-direction: column; gap: 2px; }
  .settings-link-info strong { font-size: 13px; color: var(--text-primary); }
  .settings-link-info span { font-size: 11px; color: var(--text-tertiary); }
  .settings-link-arrow { color: var(--text-tertiary); font-size: 16px; }

  /* Updates card */
  .update-version-badge { font-size: 11px; font-weight: 400; color: var(--text-tertiary); margin-left: 8px; }
  .update-toggle-label { flex: 1; }
  .update-status-area { padding: 0 16px 12px; }
  .update-banner-title { font-size: 13px; color: var(--text-primary); display: block; }
  .update-notes { font-size: 12px; color: var(--text-secondary); margin: 4px 0 0; white-space: pre-wrap; max-height: 80px; overflow-y: auto; line-height: 1.5; }
  .update-status-line { display: flex; align-items: center; gap: 8px; font-size: 12px; }
  .update-dot { width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0; }
  .update-dot-ok { background: var(--success); }
  .update-dot-error { background: var(--danger); }
  .update-status-ok { color: var(--success); }
  .update-status-error { color: var(--danger); }
</style>
