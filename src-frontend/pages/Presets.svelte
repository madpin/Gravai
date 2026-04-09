<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "../lib/tauri";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";

  let presets = $state<Record<string, any>>({});
  let activeId = $state<string | null>(null);
  let editingId = $state<string | null>(null);

  // Edit form state
  let editName = $state("");
  let editDesc = $state("");
  let editMicEnabled = $state(true);
  let editMicGain = $state(1.0);
  let editSysEnabled = $state(true);
  let editSysGain = $state(1.0);
  let editSampleRate = $state("48000");
  let editBitDepth = $state("24");
  let editChannels = $state("2");
  let editExportFormat = $state("m4a-aac");
  let editOutputFolder = $state("");

  onMount(load);

  async function load() {
    try {
      const store: any = await invoke("get_presets");
      presets = store.presets || {};
      activeId = store.active_preset_id;
    } catch (_) {}
  }

  async function activate(id: string) {
    try { await invoke("activate_preset", { presetId: id }); activeId = id; } catch (_) {}
  }

  function startEdit(id: string | null) {
    if (id && presets[id]) {
      const p = presets[id];
      editingId = id;
      editName = p.name; editDesc = p.description;
      editMicEnabled = p.mic_enabled; editMicGain = p.mic_gain;
      editSysEnabled = p.sys_enabled; editSysGain = p.sys_gain;
      editSampleRate = String(p.sample_rate); editBitDepth = String(p.bit_depth);
      editChannels = String(p.channels); editExportFormat = p.export_format;
      editOutputFolder = p.output_folder || "";
    } else {
      // New preset
      editingId = "__new__";
      editName = ""; editDesc = "";
      editMicEnabled = true; editMicGain = 1.0;
      editSysEnabled = true; editSysGain = 1.0;
      editSampleRate = "48000"; editBitDepth = "24";
      editChannels = "2"; editExportFormat = "m4a-aac";
      editOutputFolder = "";
    }
  }

  function cancelEdit() { editingId = null; }

  async function saveEdit() {
    const id = editingId === "__new__" ? crypto.randomUUID() : editingId!;
    const preset = {
      id, name: editName, description: editDesc,
      mic_enabled: editMicEnabled, mic_gain: editMicGain,
      sys_enabled: editSysEnabled, sys_gain: editSysGain,
      sample_rate: parseInt(editSampleRate), bit_depth: parseInt(editBitDepth),
      channels: parseInt(editChannels), export_format: editExportFormat,
      output_folder: editOutputFolder || null,
    };
    try {
      await invoke("save_preset", { preset });
      editingId = null;
      await load();
    } catch (_) {}
  }

  async function deletePreset(id: string) {
    if (!confirm(`Delete preset "${presets[id]?.name}"?`)) return;
    try { await invoke("delete_preset", { presetId: id }); await load(); } catch (_) {}
  }

  async function pickFolder() {
    const s = await openDialog({ directory: true, title: "Output folder" });
    if (s) editOutputFolder = s as string;
  }
</script>

<div class="page-header">
  <h2>Capture Presets</h2>
  <button class="btn btn-xs btn-accent" onclick={() => startEdit(null)}>+ New Preset</button>
</div>
<p class="page-desc">Presets control audio sources, recording quality, and output. Activate one to apply its settings.</p>

{#if editingId}
  <!-- Edit/Create form -->
  <div class="card">
    <div class="card-header">{editingId === "__new__" ? "New Preset" : `Edit: ${editName}`}</div>
    <div class="settings-grid">
      <div class="setting-row">
        <div class="setting-info"><span class="setting-label">Name</span></div>
        <input class="input" bind:value={editName} placeholder="My Preset" />
      </div>
      <div class="setting-row">
        <div class="setting-info"><span class="setting-label">Description</span></div>
        <input class="input" bind:value={editDesc} placeholder="What this preset is for" />
      </div>
      <div class="setting-row">
        <div class="setting-info"><span class="setting-label">Microphone</span><span class="setting-desc">Enable mic recording</span></div>
        <label class="switch"><input type="checkbox" bind:checked={editMicEnabled} /><span class="switch-slider"></span></label>
      </div>
      <div class="setting-row">
        <div class="setting-info"><span class="setting-label">Mic Gain</span><span class="setting-desc">Volume multiplier (1.0 = unity)</span></div>
        <input class="input input-narrow" type="number" bind:value={editMicGain} min="0" max="2" step="0.1" />
      </div>
      <div class="setting-row">
        <div class="setting-info"><span class="setting-label">System Audio</span><span class="setting-desc">Capture app audio</span></div>
        <label class="switch"><input type="checkbox" bind:checked={editSysEnabled} /><span class="switch-slider"></span></label>
      </div>
      <div class="setting-row">
        <div class="setting-info"><span class="setting-label">System Gain</span></div>
        <input class="input input-narrow" type="number" bind:value={editSysGain} min="0" max="2" step="0.1" />
      </div>
      <div class="setting-row">
        <div class="setting-info"><span class="setting-label">Sample Rate</span></div>
        <select class="select" bind:value={editSampleRate}>
          <option value="16000">16,000 Hz</option><option value="44100">44,100 Hz</option><option value="48000">48,000 Hz</option><option value="96000">96,000 Hz</option>
        </select>
      </div>
      <div class="setting-row">
        <div class="setting-info"><span class="setting-label">Bit Depth</span></div>
        <select class="select" bind:value={editBitDepth}>
          <option value="16">16-bit</option><option value="24">24-bit</option><option value="32">32-bit</option>
        </select>
      </div>
      <div class="setting-row">
        <div class="setting-info"><span class="setting-label">Channels</span></div>
        <select class="select" bind:value={editChannels}>
          <option value="1">Mono</option><option value="2">Stereo</option>
        </select>
      </div>
      <div class="setting-row">
        <div class="setting-info"><span class="setting-label">Export Format</span></div>
        <select class="select" bind:value={editExportFormat}>
          <option value="wav">WAV</option><option value="aiff">AIFF</option><option value="caf">CAF</option><option value="m4a-aac">M4A (AAC)</option><option value="m4a-alac">M4A (ALAC)</option>
        </select>
      </div>
      <div class="setting-row">
        <div class="setting-info"><span class="setting-label">Output Folder</span><span class="setting-desc">Empty = default</span></div>
        <div class="folder-row">
          <input class="input folder-input" bind:value={editOutputFolder} placeholder="Default" />
          <button class="btn btn-xs btn-ghost" onclick={pickFolder}>Browse</button>
        </div>
      </div>
    </div>
    <div class="card-footer">
      <button class="btn btn-xs btn-accent" onclick={saveEdit}>Save</button>
      <button class="btn btn-xs btn-ghost" onclick={cancelEdit}>Cancel</button>
    </div>
  </div>
{/if}

<!-- Preset cards -->
<div class="card-grid">
  {#each Object.entries(presets) as [id, p]}
    <div class="card" class:active-card={activeId === id}>
      <div class="card-header">
        {p.name}
        {#if activeId === id}<span class="card-tag card-tag-active">Active</span>{/if}
      </div>
      <div class="card-body">
        <p>{p.description}</p>
        <div class="tags-row">
          <span class="card-tag">{p.mic_enabled ? "🎤 Mic" : "🎤 Off"}</span>
          <span class="card-tag">{p.sys_enabled ? "💻 System" : "💻 Off"}</span>
          <span class="card-tag">{p.sample_rate / 1000}kHz</span>
          <span class="card-tag">{p.bit_depth}-bit</span>
          <span class="card-tag">{p.channels === 1 ? "Mono" : "Stereo"}</span>
          <span class="card-tag">{p.export_format}</span>
        </div>
      </div>
      <div class="card-actions">
        <button class="btn btn-xs btn-accent" onclick={() => activate(id)} disabled={activeId === id}>
          {activeId === id ? "Active" : "Activate"}
        </button>
        <button class="btn btn-xs btn-ghost" onclick={() => startEdit(id)}>Edit</button>
        <button class="btn btn-xs btn-ghost btn-danger" onclick={() => deletePreset(id)}>Delete</button>
      </div>
    </div>
  {/each}
</div>

<style>
  .folder-input { max-width: 180px; }
</style>
