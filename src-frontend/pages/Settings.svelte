<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "../lib/tauri";
  import { healthStatus } from "../lib/store";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";

  // Flat state for each setting — avoids nested object reactivity issues
  let recEnabled = $state(true);
  let recOutputFolder = $state("");
  let recSampleRate = $state("48000");
  let recBitDepth = $state("24");
  let recChannels = $state("2");
  let recExportFormat = $state("m4a-aac");
  let recAacBitrate = $state(192);

  let transEngine = $state("whisper");
  let transModel = $state("medium");
  let transLang = $state("en");

  let vadEngine = $state("webrtc");
  let vadPause = $state(0.5);
  let vadMin = $state(0.3);
  let vadMax = $state(30);

  let echoEnabled = $state(true);
  let echoThreshold = $state(0.55);
  let meetingEnabled = $state(true);
  let diarizationEnabled = $state(false);

  let llmProvider = $state("ollama");
  let llmModel = $state("gemma3:4b");
  let llmUrl = $state("http://localhost:11434/v1");

  let healthChecks = $state<any[]>([]);
  let rawJson = $state("");
  let saveMsg = $state("");
  let loaded = $state(false);

  onMount(async () => { await loadConfig(); await loadHealth(); });

  async function loadConfig() {
    try {
      const c: any = await invoke("get_config");
      // Populate flat state from nested config
      recEnabled = c.audio?.recording?.enabled ?? true;
      recOutputFolder = c.audio?.recording?.output_folder ?? "";
      recSampleRate = String(c.audio?.recording?.sample_rate ?? 48000);
      recBitDepth = String(c.audio?.recording?.bit_depth ?? 24);
      recChannels = String(c.audio?.recording?.channels ?? 2);
      recExportFormat = c.audio?.recording?.export_format ?? "m4a-aac";
      recAacBitrate = c.audio?.recording?.aac_bitrate_kbps ?? 192;

      transEngine = c.transcription?.engine ?? "whisper";
      transModel = c.transcription?.model ?? "medium";
      transLang = c.transcription?.language ?? "en";

      vadEngine = c.vad?.engine ?? "webrtc";
      vadPause = c.vad?.pause_seconds ?? 0.5;
      vadMin = c.vad?.silero?.min_utterance_seconds ?? 0.3;
      vadMax = c.vad?.silero?.max_utterance_seconds ?? 30;

      echoEnabled = c.features?.echo_suppression?.enabled ?? true;
      echoThreshold = c.features?.echo_suppression?.similarity_threshold ?? 0.55;
      meetingEnabled = c.features?.meeting_detection?.enabled ?? true;
      diarizationEnabled = c.features?.diarization?.enabled ?? false;

      llmProvider = c.llm?.provider ?? "ollama";
      llmModel = c.llm?.model ?? "gemma3:4b";
      llmUrl = c.llm?.base_url ?? "http://localhost:11434/v1";

      rawJson = JSON.stringify(c, null, 2);
      loaded = true;
    } catch (e) { console.error("loadConfig:", e); }
  }

  async function loadHealth() {
    try {
      const report: any = await invoke("get_health_report");
      healthChecks = report.checks || [];
      healthStatus.set(report.overall);
    } catch (_) {}
  }

  async function pickOutputFolder() {
    try {
      const selected = await openDialog({ directory: true, title: "Select output folder" });
      if (selected) { recOutputFolder = selected as string; save(); }
    } catch (_) {}
  }

  async function save() {
    try {
      const patch = {
        audio: { recording: {
          enabled: recEnabled,
          output_folder: recOutputFolder || null,
          sample_rate: parseInt(recSampleRate),
          bit_depth: parseInt(recBitDepth),
          channels: parseInt(recChannels),
          export_format: recExportFormat,
          aac_bitrate_kbps: recAacBitrate,
        }},
        transcription: { engine: transEngine, model: transModel, language: transLang },
        vad: { engine: vadEngine, pause_seconds: vadPause, silero: { min_utterance_seconds: vadMin, max_utterance_seconds: vadMax } },
        features: {
          echo_suppression: { enabled: echoEnabled, similarity_threshold: echoThreshold },
          meeting_detection: { enabled: meetingEnabled },
          diarization: { enabled: diarizationEnabled },
        },
        llm: { provider: llmProvider, model: llmModel, base_url: llmUrl },
      };
      const updated: any = await invoke("update_config", { patch });
      rawJson = JSON.stringify(updated, null, 2);
      saveMsg = "Saved!"; setTimeout(() => saveMsg = "", 1500);
    } catch (e: any) { saveMsg = `Error: ${e}`; }
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
      try { await invoke("import_config", { json: t }); await loadConfig(); saveMsg = "Imported!"; setTimeout(() => saveMsg = "", 1500); } catch (e: any) { saveMsg = `Error: ${e}`; }
    };
    input.click();
  }

  async function saveRawJson() {
    try { await invoke("import_config", { json: rawJson }); await loadConfig(); saveMsg = "JSON saved!"; setTimeout(() => saveMsg = "", 1500); } catch (e: any) { saveMsg = `Error: ${e}`; }
  }
</script>

<div class="page-header">
  <h2>Settings</h2>
  <div class="header-actions">
    {#if saveMsg}<span style="font-size:11px;color:var(--success)">{saveMsg}</span>{/if}
    <button class="btn btn-xs btn-ghost" onclick={importConfig}>Import</button>
    <button class="btn btn-xs btn-ghost" onclick={exportConfig}>Export</button>
  </div>
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
  </div>
</div>

{#if loaded}
<!-- Audio -->
<div class="card">
  <div class="card-header">Audio Recording</div>
  <div class="settings-grid">
    <div class="setting-row">
      <div class="setting-info"><span class="setting-label">Save Recordings</span><span class="setting-desc">Write audio files to disk (disable for transcription-only mode)</span></div>
      <label class="switch"><input type="checkbox" bind:checked={recEnabled} onchange={save} /><span class="switch-slider"></span></label>
    </div>
    <div class="setting-row">
      <div class="setting-info"><span class="setting-label">Output Folder</span><span class="setting-desc">Where to save recordings (empty = default ~/.gravai/sessions/)</span></div>
      <div style="display:flex;gap:6px;align-items:center">
        <input class="input" bind:value={recOutputFolder} onchange={save} placeholder="Default" style="max-width:200px" />
        <button class="btn btn-xs btn-ghost" onclick={pickOutputFolder}>Browse</button>
        {#if recOutputFolder}<button class="btn btn-xs btn-ghost" onclick={() => { recOutputFolder = ""; save(); }}>Clear</button>{/if}
      </div>
    </div>
    <div class="setting-row">
      <div class="setting-info"><span class="setting-label">Sample Rate</span><span class="setting-desc">Higher values = better quality, larger files</span></div>
      <select class="select" bind:value={recSampleRate} onchange={save}>
        <option value="16000">16,000 Hz</option><option value="44100">44,100 Hz</option><option value="48000">48,000 Hz</option><option value="96000">96,000 Hz</option>
      </select>
    </div>
    <div class="setting-row">
      <div class="setting-info"><span class="setting-label">Bit Depth</span><span class="setting-desc">Dynamic range per sample</span></div>
      <select class="select" bind:value={recBitDepth} onchange={save}>
        <option value="16">16-bit</option><option value="24">24-bit</option><option value="32">32-bit</option>
      </select>
    </div>
    <div class="setting-row">
      <div class="setting-info"><span class="setting-label">Channels</span><span class="setting-desc">Mono (1 ch) or Stereo (2 ch)</span></div>
      <select class="select" bind:value={recChannels} onchange={save}>
        <option value="1">Mono</option><option value="2">Stereo</option>
      </select>
    </div>
    <div class="setting-row">
      <div class="setting-info"><span class="setting-label">Export Format</span><span class="setting-desc">Default audio file format</span></div>
      <select class="select" bind:value={recExportFormat} onchange={save}>
        <option value="wav">WAV (lossless, large)</option><option value="aiff">AIFF (lossless)</option><option value="caf">CAF (Core Audio)</option><option value="m4a-aac">M4A AAC (compressed, small)</option><option value="m4a-alac">M4A ALAC (lossless, small)</option>
      </select>
    </div>
    <div class="setting-row">
      <div class="setting-info"><span class="setting-label">AAC Bitrate</span><span class="setting-desc">For M4A AAC export (higher = better quality)</span></div>
      <input class="input input-narrow" type="number" bind:value={recAacBitrate} onchange={save} min="64" max="320" step="32" /> <span style="font-size:11px;color:var(--text-tertiary)">kbps</span>
    </div>
  </div>
</div>

<!-- Transcription -->
<div class="card">
  <div class="card-header">Transcription</div>
  <div class="settings-grid">
    <div class="setting-row">
      <div class="setting-info"><span class="setting-label">Engine</span><span class="setting-desc">Where transcription runs</span></div>
      <select class="select" bind:value={transEngine} onchange={save}>
        <option value="whisper">Whisper (on-device)</option><option value="http">External HTTP API</option>
      </select>
    </div>
    <div class="setting-row">
      <div class="setting-info"><span class="setting-label">Model Size</span><span class="setting-desc">Larger models are more accurate but slower and use more RAM</span></div>
      <select class="select" bind:value={transModel} onchange={save}>
        <option value="tiny">Tiny (75 MB, fastest)</option><option value="base">Base (142 MB)</option><option value="small">Small (466 MB)</option><option value="medium">Medium (1.5 GB, balanced)</option><option value="large">Large (3 GB, best accuracy)</option>
      </select>
    </div>
    <div class="setting-row">
      <div class="setting-info"><span class="setting-label">Language</span><span class="setting-desc">Primary language for transcription</span></div>
      <select class="select" bind:value={transLang} onchange={save}>
        <option value="en">English</option><option value="pt">Portuguese</option><option value="es">Spanish</option><option value="fr">French</option><option value="de">German</option><option value="pl">Polish</option><option value="ja">Japanese</option><option value="auto">Auto-detect</option>
      </select>
    </div>
  </div>
</div>

<!-- VAD -->
<div class="card">
  <div class="card-header">Voice Activity Detection</div>
  <div class="settings-grid">
    <div class="setting-row">
      <div class="setting-info"><span class="setting-label">VAD Engine</span><span class="setting-desc">Algorithm to detect when someone is speaking</span></div>
      <select class="select" bind:value={vadEngine} onchange={save}>
        <option value="webrtc">WebRTC (fast, low CPU)</option><option value="silero">Silero (accurate, uses ONNX)</option>
      </select>
    </div>
    <div class="setting-row">
      <div class="setting-info"><span class="setting-label">Pause Duration</span><span class="setting-desc">How long to wait after silence before finalizing an utterance</span></div>
      <input class="input input-narrow" type="number" bind:value={vadPause} onchange={save} min="0.1" max="5" step="0.1" /> <span style="font-size:11px;color:var(--text-tertiary)">sec</span>
    </div>
    <div class="setting-row">
      <div class="setting-info"><span class="setting-label">Min Utterance Length</span><span class="setting-desc">Ignore speech segments shorter than this</span></div>
      <input class="input input-narrow" type="number" bind:value={vadMin} onchange={save} min="0.1" max="5" step="0.1" /> <span style="font-size:11px;color:var(--text-tertiary)">sec</span>
    </div>
    <div class="setting-row">
      <div class="setting-info"><span class="setting-label">Max Utterance Length</span><span class="setting-desc">Force-split long speech after this duration</span></div>
      <input class="input input-narrow" type="number" bind:value={vadMax} onchange={save} min="5" max="120" step="5" /> <span style="font-size:11px;color:var(--text-tertiary)">sec</span>
    </div>
  </div>
</div>

<!-- Features -->
<div class="card">
  <div class="card-header">Features</div>
  <div class="settings-grid">
    <div class="setting-row">
      <div class="setting-info"><span class="setting-label">Echo Suppression</span><span class="setting-desc">Prevents the same speech from appearing twice when captured by both mic and system audio</span></div>
      <label class="switch"><input type="checkbox" bind:checked={echoEnabled} onchange={save} /><span class="switch-slider"></span></label>
    </div>
    <div class="setting-row">
      <div class="setting-info"><span class="setting-label">Echo Threshold</span><span class="setting-desc">Similarity score to consider two utterances as duplicates (0.0 - 1.0)</span></div>
      <input class="input input-narrow" type="number" bind:value={echoThreshold} onchange={save} min="0" max="1" step="0.05" />
    </div>
    <div class="setting-row">
      <div class="setting-info"><span class="setting-label">Meeting Detection</span><span class="setting-desc">Automatically detect when a meeting app (Zoom, Teams, etc.) is running</span></div>
      <label class="switch"><input type="checkbox" bind:checked={meetingEnabled} onchange={save} /><span class="switch-slider"></span></label>
    </div>
    <div class="setting-row">
      <div class="setting-info"><span class="setting-label">Speaker Diarization</span><span class="setting-desc">Identify different speakers in system audio (remote participants)</span></div>
      <label class="switch"><input type="checkbox" bind:checked={diarizationEnabled} onchange={save} /><span class="switch-slider"></span></label>
    </div>
  </div>
</div>

<!-- LLM -->
<div class="card">
  <div class="card-header">AI / LLM</div>
  <div class="settings-grid">
    <div class="setting-row">
      <div class="setting-info"><span class="setting-label">Provider</span><span class="setting-desc">Which LLM service to use for summaries and analysis</span></div>
      <select class="select" bind:value={llmProvider} onchange={save}>
        <option value="ollama">Ollama (local, free)</option><option value="openai">OpenAI (BYOK)</option><option value="anthropic">Anthropic (BYOK)</option>
      </select>
    </div>
    <div class="setting-row">
      <div class="setting-info"><span class="setting-label">Model</span><span class="setting-desc">Model name to use (e.g. gemma3:4b for Ollama, gpt-4o-mini for OpenAI)</span></div>
      <input class="input" bind:value={llmModel} onchange={save} />
    </div>
    <div class="setting-row">
      <div class="setting-info"><span class="setting-label">API Base URL</span><span class="setting-desc">Endpoint for the LLM service</span></div>
      <input class="input" bind:value={llmUrl} onchange={save} />
    </div>
  </div>
</div>
{/if}

<!-- Advanced JSON -->
<details class="card collapsible">
  <summary class="card-header">Advanced: Raw Config JSON</summary>
  <textarea class="config-editor" rows="16" spellcheck="false" bind:value={rawJson}></textarea>
  <div class="card-footer"><button class="btn btn-xs btn-accent" onclick={saveRawJson}>Save JSON</button></div>
</details>
