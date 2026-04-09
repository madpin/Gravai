<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "../lib/tauri";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";

  let profiles = $state<Record<string, any>>({});
  let activeId = $state<string | null>(null);
  let editingId = $state<string | null>(null);
  let saveMsg = $state("");

  // Edit form — mirrors the full profile structure
  let e = $state({
    name: "", description: "",
    // Transcription
    trans_engine: "whisper", trans_model: "medium", trans_lang: "en",
    // VAD
    vad_engine: "webrtc", vad_pause: 0.5, vad_min: 0.3, vad_max: 30,
    // Features
    echo_enabled: true, echo_threshold: 0.55,
    meeting_enabled: true, diarization_enabled: false, sentiment_enabled: false,
    // LLM
    llm_provider: "ollama", llm_model: "gemma3:4b", llm_url: "http://localhost:11434/v1",
    // Export
    auto_export_transcript: false, auto_export_audio: false,
    transcript_folder: "", audio_folder: "",
    transcript_format: "markdown", realtime_save: true,
  });

  onMount(load);

  async function load() {
    try {
      const ps: any = await invoke("get_profiles");
      profiles = ps.profiles || {}; activeId = ps.active_profile_id;
    } catch (_) {}
  }

  async function activate(id: string) {
    try {
      await invoke("activate_profile", { profileId: id });
      activeId = id;
      // Apply profile settings to main config
      const p = profiles[id];
      if (p) {
        const patch: any = {};
        if (p.transcription_engine) patch.transcription = { ...patch.transcription, engine: p.transcription_engine };
        if (p.transcription_model) patch.transcription = { ...patch.transcription, model: p.transcription_model };
        if (p.transcription_language) patch.transcription = { ...patch.transcription, language: p.transcription_language };
        if (p.llm_provider) patch.llm = { ...patch.llm, provider: p.llm_provider };
        if (p.llm_model) patch.llm = { ...patch.llm, model: p.llm_model };
        if (p.diarization_enabled != null) patch.features = { ...patch.features, diarization: { enabled: p.diarization_enabled } };
        if (p.echo_suppression_enabled != null) patch.features = { ...patch.features, echo_suppression: { enabled: p.echo_suppression_enabled } };
        if (Object.keys(patch).length > 0) await invoke("update_config", { patch });
      }
    } catch (_) {}
  }

  function startEdit(id: string | null) {
    if (id && profiles[id]) {
      const p = profiles[id];
      editingId = id;
      e.name = p.name || ""; e.description = p.description || "";
      e.trans_engine = p.transcription_engine || "whisper";
      e.trans_model = p.transcription_model || "medium";
      e.trans_lang = p.transcription_language || "en";
      e.vad_engine = p.vad_engine || "webrtc";
      e.vad_pause = p.vad_pause ?? 0.5; e.vad_min = p.vad_min ?? 0.3; e.vad_max = p.vad_max ?? 30;
      e.echo_enabled = p.echo_suppression_enabled ?? true;
      e.echo_threshold = p.echo_threshold ?? 0.55;
      e.meeting_enabled = p.meeting_enabled ?? true;
      e.diarization_enabled = p.diarization_enabled ?? false;
      e.sentiment_enabled = p.sentiment_enabled ?? false;
      e.llm_provider = p.llm_provider || "ollama";
      e.llm_model = p.llm_model || "gemma3:4b";
      e.llm_url = p.llm_url || "http://localhost:11434/v1";
      e.auto_export_transcript = p.auto_export_transcript ?? false;
      e.auto_export_audio = p.auto_export_audio ?? false;
      e.transcript_folder = p.transcript_folder || "";
      e.audio_folder = p.audio_folder || "";
      e.transcript_format = p.transcript_format || "markdown";
      e.realtime_save = p.realtime_save ?? true;
    } else {
      editingId = "__new__";
      e = { name: "", description: "",
        trans_engine: "whisper", trans_model: "medium", trans_lang: "en",
        vad_engine: "webrtc", vad_pause: 0.5, vad_min: 0.3, vad_max: 30,
        echo_enabled: true, echo_threshold: 0.55,
        meeting_enabled: true, diarization_enabled: false, sentiment_enabled: false,
        llm_provider: "ollama", llm_model: "gemma3:4b", llm_url: "http://localhost:11434/v1",
        auto_export_transcript: false, auto_export_audio: false,
        transcript_folder: "", audio_folder: "", transcript_format: "markdown", realtime_save: true,
      };
    }
  }

  function cancelEdit() { editingId = null; }

  async function saveEdit() {
    const id = editingId === "__new__" ? crypto.randomUUID() : editingId!;
    const profile = {
      id, name: e.name, description: e.description, preset_id: null,
      transcription_engine: e.trans_engine, transcription_model: e.trans_model,
      transcription_language: e.trans_lang,
      vad_engine: e.vad_engine, vad_pause: e.vad_pause, vad_min: e.vad_min, vad_max: e.vad_max,
      echo_suppression_enabled: e.echo_enabled, echo_threshold: e.echo_threshold,
      meeting_enabled: e.meeting_enabled, diarization_enabled: e.diarization_enabled, sentiment_enabled: e.sentiment_enabled,
      llm_provider: e.llm_provider, llm_model: e.llm_model, llm_url: e.llm_url,
      auto_export_transcript: e.auto_export_transcript, auto_export_audio: e.auto_export_audio,
      transcript_folder: e.transcript_folder || null, audio_folder: e.audio_folder || null,
      transcript_format: e.transcript_format, realtime_save: e.realtime_save,
      shortcut_set_id: null,
    };
    try {
      await invoke("save_profile", { profile });
      editingId = null;
      await load();
    } catch (err) { saveMsg = `Error: ${err}`; }
  }
</script>

<div class="page-header">
  <h2>Profiles</h2>
  <button class="btn btn-xs btn-accent" onclick={() => startEdit(null)}>+ New Profile</button>
</div>
<p class="page-desc">Profiles bundle transcription, AI, features, and export settings. Activate a profile to apply all its settings at once.</p>

{#if editingId}
  <div class="card">
    <div class="card-header">{editingId === "__new__" ? "New Profile" : `Edit: ${e.name}`}</div>
    <div class="settings-grid">
      <!-- Basic -->
      <div class="setting-row"><div class="setting-info"><span class="setting-label">Name</span></div><input class="input" bind:value={e.name} placeholder="My Profile" /></div>
      <div class="setting-row"><div class="setting-info"><span class="setting-label">Description</span></div><input class="input" bind:value={e.description} placeholder="Context for this profile" /></div>
    </div>

    <!-- Transcription -->
    <div class="card-header section">Transcription</div>
    <div class="settings-grid">
      <div class="setting-row"><div class="setting-info"><span class="setting-label">Engine</span></div>
        <select class="select" bind:value={e.trans_engine}><option value="whisper">Whisper</option><option value="http">External HTTP</option></select></div>
      <div class="setting-row"><div class="setting-info"><span class="setting-label">Model</span><span class="setting-desc">Larger = more accurate, slower. <button class="btn-link" onclick={() => { import("../lib/store").then(m => m.currentPage.set("models")); }}>Manage Models →</button></span></div>
        <select class="select" bind:value={e.trans_model}><option value="tiny">Tiny</option><option value="base">Base</option><option value="small">Small</option><option value="medium">Medium</option><option value="large-v3-turbo">Large v3 Turbo ⚡</option><option value="large-v3">Large v3</option></select></div>
      <div class="setting-row"><div class="setting-info"><span class="setting-label">Language</span></div>
        <select class="select" bind:value={e.trans_lang}><option value="en">English</option><option value="pt">Portuguese</option><option value="es">Spanish</option><option value="fr">French</option><option value="de">German</option><option value="pl">Polish</option><option value="ja">Japanese</option><option value="auto">Auto</option></select></div>
    </div>

    <!-- VAD -->
    <div class="card-header section">Voice Activity Detection</div>
    <div class="settings-grid">
      <div class="setting-row"><div class="setting-info"><span class="setting-label">Engine</span></div>
        <select class="select" bind:value={e.vad_engine}><option value="webrtc">WebRTC</option><option value="silero">Silero</option></select></div>
      <div class="setting-row"><div class="setting-info"><span class="setting-label">Pause</span><span class="setting-desc">Seconds of silence to finalize</span></div><input class="input input-narrow" type="number" bind:value={e.vad_pause} min="0.1" max="5" step="0.1" /></div>
      <div class="setting-row"><div class="setting-info"><span class="setting-label">Min Utterance</span></div><input class="input input-narrow" type="number" bind:value={e.vad_min} min="0.1" max="5" step="0.1" /></div>
      <div class="setting-row"><div class="setting-info"><span class="setting-label">Max Utterance</span></div><input class="input input-narrow" type="number" bind:value={e.vad_max} min="5" max="120" step="5" /></div>
    </div>

    <!-- Features -->
    <div class="card-header section">Features</div>
    <div class="settings-grid">
      <div class="setting-row"><div class="setting-info"><span class="setting-label">Echo Suppression</span></div><label class="switch"><input type="checkbox" bind:checked={e.echo_enabled} /><span class="switch-slider"></span></label></div>
      <div class="setting-row"><div class="setting-info"><span class="setting-label">Meeting Detection</span></div><label class="switch"><input type="checkbox" bind:checked={e.meeting_enabled} /><span class="switch-slider"></span></label></div>
      <div class="setting-row"><div class="setting-info"><span class="setting-label">Diarization</span><span class="setting-desc">Speaker labels (You / Remote)</span></div><label class="switch"><input type="checkbox" bind:checked={e.diarization_enabled} /><span class="switch-slider"></span></label></div>
      <div class="setting-row"><div class="setting-info"><span class="setting-label">Emotion Detection</span><span class="setting-desc">Show emotions on system audio (requires go-emotions model)</span></div><label class="switch"><input type="checkbox" bind:checked={e.sentiment_enabled} /><span class="switch-slider"></span></label></div>
    </div>

    <!-- LLM -->
    <div class="card-header section">AI / LLM</div>
    <div class="settings-grid">
      <div class="setting-row"><div class="setting-info"><span class="setting-label">Provider</span></div>
        <select class="select" bind:value={e.llm_provider}><option value="ollama">Ollama</option><option value="openai">OpenAI</option><option value="anthropic">Anthropic</option></select></div>
      <div class="setting-row"><div class="setting-info"><span class="setting-label">Model</span></div><input class="input" bind:value={e.llm_model} /></div>
      <div class="setting-row"><div class="setting-info"><span class="setting-label">API URL</span></div><input class="input" bind:value={e.llm_url} /></div>
    </div>

    <!-- Export -->
    <div class="card-header section">Export</div>
    <div class="settings-grid">
      <div class="setting-row"><div class="setting-info"><span class="setting-label">Auto-Export Transcript</span></div><label class="switch"><input type="checkbox" bind:checked={e.auto_export_transcript} /><span class="switch-slider"></span></label></div>
      <div class="setting-row"><div class="setting-info"><span class="setting-label">Auto-Export Audio</span></div><label class="switch"><input type="checkbox" bind:checked={e.auto_export_audio} /><span class="switch-slider"></span></label></div>
      <div class="setting-row"><div class="setting-info"><span class="setting-label">Transcript Folder</span></div>
        <div class="folder-row"><input class="input folder-input" bind:value={e.transcript_folder} placeholder="Default" /><button class="btn btn-xs btn-ghost" onclick={async () => { const s = await openDialog({ directory: true }); if (s) e.transcript_folder = s as string; }}>Browse</button></div></div>
      <div class="setting-row"><div class="setting-info"><span class="setting-label">Audio Folder</span></div>
        <div class="folder-row"><input class="input folder-input" bind:value={e.audio_folder} placeholder="Default" /><button class="btn btn-xs btn-ghost" onclick={async () => { const s = await openDialog({ directory: true }); if (s) e.audio_folder = s as string; }}>Browse</button></div></div>
      <div class="setting-row"><div class="setting-info"><span class="setting-label">Real-Time Auto-Save</span><span class="setting-desc">Save transcript every 30s (crash-safe)</span></div><label class="switch"><input type="checkbox" bind:checked={e.realtime_save} /><span class="switch-slider"></span></label></div>
    </div>

    <div class="card-footer">
      <button class="btn btn-xs btn-accent" onclick={saveEdit}>Save Profile</button>
      <button class="btn btn-xs btn-ghost" onclick={cancelEdit}>Cancel</button>
    </div>
  </div>
{/if}

<!-- Profile cards -->
<div class="card-grid">
  {#each Object.entries(profiles) as [id, p]}
    <div class="card" class:active-card={activeId === id}>
      <div class="card-header">
        {p.name}
        {#if activeId === id}<span class="card-tag card-tag-active">Active</span>{/if}
      </div>
      <div class="card-body">
        <p>{p.description}</p>
        <div class="tags-row">
          {#if p.transcription_model}<span class="card-tag">Whisper: {p.transcription_model}</span>{/if}
          {#if p.transcription_language}<span class="card-tag">Lang: {p.transcription_language}</span>{/if}
          {#if p.diarization_enabled}<span class="card-tag">Diarization</span>{/if}
          {#if p.sentiment_enabled}<span class="card-tag">Emotions</span>{/if}
          {#if p.llm_provider}<span class="card-tag">LLM: {p.llm_provider}</span>{/if}
        </div>
      </div>
      <div class="card-actions">
        <button class="btn btn-xs btn-accent" onclick={() => activate(id)} disabled={activeId === id}>
          {activeId === id ? "Active" : "Activate"}
        </button>
        <button class="btn btn-xs btn-ghost" onclick={() => startEdit(id)}>Edit</button>
      </div>
    </div>
  {/each}
</div>

<style>
  .folder-input { max-width: 180px; }
</style>
