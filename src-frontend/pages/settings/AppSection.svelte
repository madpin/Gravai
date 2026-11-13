<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke, listen } from "../../lib/tauri";
  import { config, setValue, loadConfig } from "../../lib/config";
  import { healthStatus } from "../../lib/store";
  import Group from "../../components/settings/Group.svelte";
  import Row from "../../components/settings/Row.svelte";
  import Toggle from "../../components/settings/Toggle.svelte";
  import Segmented from "../../components/settings/Segmented.svelte";
  import More from "../../components/settings/More.svelte";
  import Onboarding from "../../components/Onboarding.svelte";

  let c = $derived($config);

  // ── Shortcuts ────────────────────────────────────────────────────────
  const ACTION_ORDER = ["recording.start", "recording.stop", "recording.pause", "recording.bookmark"];
  let bindings = $state<Record<string, any>>({});
  let capturing = $state<string | null>(null);
  let shortcutMsg = $state("");

  async function loadShortcuts() {
    try { bindings = ((await invoke("get_shortcuts")) as any).bindings ?? {}; } catch (_) {}
  }

  async function saveBinding(actionId: string, key: string, isGlobal: boolean) {
    shortcutMsg = "";
    try {
      await invoke("rebind_shortcut", { actionId, keySequence: key, isGlobal });
    } catch (e) { shortcutMsg = String(e); }
    await loadShortcuts();
    window.dispatchEvent(new CustomEvent("gravai-shortcuts-changed"));
  }

  function capture(e: KeyboardEvent, actionId: string) {
    if (capturing !== actionId) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") { capturing = null; return; }
    if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;
    if (!(e.metaKey || e.ctrlKey || e.altKey)) { shortcutMsg = "Include ⌘, ⌃, or ⌥ so it doesn't clash with typing."; return; }
    const parts: string[] = [];
    if (e.metaKey || e.ctrlKey) parts.push("CmdOrCtrl");
    if (e.altKey) parts.push("Alt");
    if (e.shiftKey) parts.push("Shift");
    const named: Record<string, string> = { " ": "Space", ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right" };
    // e.code keeps the letter when Option changes the character (⌥R = ®).
    const key = named[e.key] ?? (e.code.startsWith("Key") ? e.code.slice(3) : e.code.startsWith("Digit") ? e.code.slice(5) : e.key.toUpperCase());
    parts.push(key);
    capturing = null;
    saveBinding(actionId, parts.join("+"), bindings[actionId]?.is_global ?? true);
  }

  function pretty(seq: string): string {
    if (!seq) return "Off";
    const sym: Record<string, string> = { CmdOrCtrl: "⌘", Alt: "⌥", Shift: "⇧", Ctrl: "⌃", Space: "Space" };
    return seq.split("+").map(p => sym[p] ?? p).join("");
  }

  // ── Startup ──────────────────────────────────────────────────────────
  // Read from macOS, not the config, so it matches the Login Items.
  let launchAtLogin = $state(false);
  let startupMsg = $state("");
  async function setLaunchAtLogin(enabled: boolean) {
    startupMsg = "";
    try { launchAtLogin = await invoke("set_launch_at_login", { enabled }); }
    catch (e) { startupMsg = `Could not change it: ${e}`; }
  }

  // ── Updates ──────────────────────────────────────────────────────────
  let version = $state("");
  let checking = $state(false);
  let installing = $state(false);
  let updateInfo = $state<any>(null);
  let unlistenUpdate: (() => void) | null = null;

  async function checkUpdate() {
    checking = true; updateInfo = null;
    try { updateInfo = await invoke("check_for_update"); } catch (e) { updateInfo = { error: String(e) }; }
    checking = false;
  }
  async function install() {
    installing = true;
    try { await invoke("install_update"); } catch (e) { updateInfo = { error: String(e) }; installing = false; }
  }

  // ── Health ───────────────────────────────────────────────────────────
  let checks = $state<any[]>([]);
  let perf = $state<any>(null);
  let showWizard = $state(false);
  async function loadHealth() {
    try {
      const report: any = await invoke("get_health_report");
      checks = report.checks ?? [];
      healthStatus.set(report.overall);
    } catch (_) {}
    try { perf = await invoke("get_perf_snapshot"); } catch (_) {}
  }

  const CHECK_NAMES: Record<string, string> = {
    platform: "Platform", storage: "Storage", microphone_permission: "Microphone access",
    screen_recording_permission: "System audio access", audio_devices: "Audio inputs",
    transcription_model: "Whisper model", diarization_model: "Speaker labels", vad_model: "Voice detection",
  };

  // ── Config file ──────────────────────────────────────────────────────
  let rawJson = $state("");
  let fileMsg = $state("");
  function flash(msg: string) { fileMsg = msg; setTimeout(() => (fileMsg = ""), 2500); }

  async function exportConfig() {
    const json: string = await invoke("export_config");
    const a = document.createElement("a");
    a.href = URL.createObjectURL(new Blob([json], { type: "application/json" }));
    a.download = "gravai-config.json";
    a.click();
  }
  function importConfig() {
    const input = document.createElement("input");
    input.type = "file"; input.accept = ".json";
    input.onchange = async () => {
      if (!input.files?.length) return;
      try { await invoke("import_config", { json: await input.files[0].text() }); await loadConfig(); flash("Settings imported."); }
      catch (e) { flash(`Could not import: ${e}`); }
    };
    input.click();
  }
  async function saveRaw() {
    try { await invoke("import_config", { json: rawJson }); await loadConfig(); flash("Saved."); }
    catch (e) { flash(`Not saved: ${e}`); }
  }

  onMount(async () => {
    loadShortcuts();
    invoke("get_launch_at_login").then((v: boolean) => (launchAtLogin = v)).catch(() => {});
    loadHealth();
    try { version = await invoke("get_app_version"); } catch (_) {}
    unlistenUpdate = await listen("gravai:update-available", (e: any) => { updateInfo = { ...e.payload, available: true }; });
  });
  onDestroy(() => unlistenUpdate?.());

  $effect(() => { if (c) rawJson = JSON.stringify(c, null, 2); });
</script>

{#if showWizard}<Onboarding onComplete={() => (showWizard = false)} />{/if}

<Group title="Keyboard shortcuts" note="Anywhere shortcuts work while another app is in front. Click a shortcut, then press the new keys.">
  {#each ACTION_ORDER.filter(id => bindings[id]) as id (id)}
    {@const b = bindings[id]}
    <Row label={b.description}>
      <button
        class="key"
        class:capturing={capturing === id}
        onclick={() => { capturing = id; shortcutMsg = ""; }}
        onkeydown={(e) => capture(e, id)}
        onblur={() => { if (capturing === id) capturing = null; }}
      >{capturing === id ? "Press keys…" : pretty(b.key_sequence)}</button>
      {#if b.key_sequence}
        <button class="clear" title="Turn off" aria-label="Turn off {b.description}" onclick={() => saveBinding(id, "", b.is_global)}>×</button>
      {/if}
      <label class="scope" title="Works while another app is in front">
        <Toggle label="Anywhere" checked={b.is_global} onchange={(v) => saveBinding(id, b.key_sequence, v)} />
        <span>Anywhere</span>
      </label>
    </Row>
  {/each}
  {#if shortcutMsg}<p class="msg err">{shortcutMsg}</p>{/if}
</Group>

<Group title="Startup">
  <Row label="Open when you log in" hint="Gravai starts by itself when you turn on your Mac.">
    <Toggle label="Open when you log in" checked={launchAtLogin} onchange={setLaunchAtLogin} />
  </Row>
  {#if c && launchAtLogin}
    <Row label="Open as" hint={c.startup.window === "menu_bar" ? "Click the menu bar icon or the Dock icon to show the window." : ""}>
      <Segmented
        options={[["window", "Window"], ["minimized", "Minimized"], ["menu_bar", "Menu bar only"]]}
        value={c.startup.window}
        onchange={(v) => setValue("startup.window", v)}
      />
    </Row>
  {/if}
  {#if startupMsg}<p class="msg err">{startupMsg}</p>{/if}
</Group>

<Group title="Updates">
  <Row label="Check when Gravai opens" hint={version ? `You have version ${version}.` : ""}>
    <button class="btn btn-xs btn-ghost" onclick={checkUpdate} disabled={checking}>{checking ? "Checking…" : "Check now"}</button>
    {#if c}<Toggle label="Check when Gravai opens" checked={c.updates.auto_check} onchange={(v) => setValue("updates.auto_check", v)} />{/if}
  </Row>
  {#if updateInfo?.available}
    <div class="update">
      <span><strong>Version {updateInfo.version} is available.</strong>{#if updateInfo.body} {updateInfo.body}{/if}</span>
      <button class="btn btn-xs btn-accent" onclick={install} disabled={installing}>{installing ? "Installing…" : "Install and restart"}</button>
    </div>
  {:else if updateInfo?.error}
    <p class="msg err">Could not check: {updateInfo.error}</p>
  {:else if updateInfo}
    <p class="msg">You're up to date.</p>
  {/if}
</Group>

<Group title="This Mac">
  {#each checks as check}
    <div class="health">
      <span class="dot {check.status}"></span>
      <span class="h-name">{CHECK_NAMES[check.name] ?? check.name.replace(/_/g, " ")}</span>
      <span class="h-msg">{check.message}</span>
    </div>
  {/each}
  {#if perf}
    <div class="health">
      <span class="dot ok"></span>
      <span class="h-name">Memory</span>
      <span class="h-msg">Gravai uses {perf.rss_mb.toFixed(0)} MB of {perf.total_memory_gb.toFixed(0)} GB</span>
    </div>
  {/if}
  <div class="health-actions">
    <button class="btn btn-xs btn-ghost" onclick={loadHealth}>Check again</button>
    <button class="btn btn-xs btn-ghost" onclick={() => (showWizard = true)}>Run setup guide</button>
  </div>
</Group>

<Group title="Settings file" note="Move your settings to another Mac, or keep a backup.">
  <Row label="Export or import">
    {#if fileMsg}<span class="msg inline">{fileMsg}</span>{/if}
    <button class="btn btn-xs btn-ghost" onclick={importConfig}>Import…</button>
    <button class="btn btn-xs btn-ghost" onclick={exportConfig}>Export…</button>
  </Row>
  <More label="Edit as JSON">
    <div class="raw">
      <textarea class="config-editor" rows="16" spellcheck="false" bind:value={rawJson}></textarea>
      <div class="raw-actions"><button class="btn btn-xs btn-accent" onclick={saveRaw}>Save JSON</button></div>
    </div>
  </More>
</Group>

<style>
  .key {
    min-width: 86px; padding: 4px 10px; border-radius: 6px; font: inherit; font-size: 13px; letter-spacing: 1px;
    background: var(--bg-base); color: var(--text-primary); border: 1px solid var(--border); cursor: pointer;
  }
  .key.capturing { border-color: var(--accent); color: var(--accent); letter-spacing: normal; font-size: 12px; }
  .clear { background: none; border: none; color: var(--text-tertiary); cursor: pointer; font-size: 16px; padding: 0 4px; }
  .clear:hover { color: var(--danger); }
  .scope { display: flex; align-items: center; gap: 6px; font-size: 11px; color: var(--text-tertiary); margin-left: 8px; }
  .msg { margin: 0; padding: 8px 18px; font-size: 12px; color: var(--text-secondary); }
  .msg.err { color: var(--danger); }
  .msg.inline { padding: 0; }
  .update { display: flex; align-items: center; gap: 12px; padding: 10px 18px; font-size: 12px; color: var(--text-secondary); border-top: 1px solid var(--border-subtle); }
  .update span { flex: 1; }
  .health { display: grid; grid-template-columns: 10px 150px 1fr; align-items: center; gap: 10px; padding: 8px 18px; border-bottom: 1px solid var(--border-subtle); font-size: 12px; }
  .h-name { color: var(--text-primary); }
  .h-msg { color: var(--text-tertiary); }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--text-tertiary); }
  .dot.ok { background: var(--success); }
  .dot.warn, .dot.warning { background: var(--warning); }
  .dot.error, .dot.fail { background: var(--danger); }
  .health-actions { display: flex; gap: 6px; padding: 8px 14px; }
  .raw { padding: 12px 18px; display: flex; flex-direction: column; gap: 8px; }
  .raw textarea { width: 100%; box-sizing: border-box; font-family: "SF Mono", monospace; font-size: 11px; }
  .raw-actions { display: flex; justify-content: flex-end; }
</style>
