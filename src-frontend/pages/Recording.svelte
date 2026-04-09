<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke, listen, fmtTimer, fmtDuration } from "../lib/tauri";
  import { get } from "svelte/store";
  import { isRecording, isPaused, currentSessionId, sessionStartTime, autoScrollEnabled, liveUtterances, lastSessionId as lastSessionIdStore, activityLogs, liveSummary, dismissedMeetingApps, addAlert, dismissAlertsByLevel, clearAlerts, currentPage } from "../lib/store";
  import TranscriptView from "../components/TranscriptView.svelte";
  import AppPicker from "../components/AppPicker.svelte";

  let timer = $state("");
  let micEnabled = $state(true);
  let sysEnabled = $state(true);
  let micVolume = $state(100);
  let sysVolume = $state(100);
  let vuMic = $state(0);
  let vuSys = $state(0);
  let summaryLoading = $state(false);
  let sentimentData = $state<any>(null);

  // Device selection
  let micDevices = $state<any[]>([]);
  let selectedMicIndex = $state(-1);
  let runningApps = $state<any[]>([]);
  let selectedAppBundleId = $state("");

  // Export config (loaded once for display)
  let exportAutoTranscript = $state(false);
  let exportAutoAudio = $state(false);
  let exportRealtimeSave = $state(true);

  // Active preset/profile info
  let activePreset = $state<any>(null);
  let activeProfile = $state<any>(null);
  let showEmotions = $derived(activeProfile?.sentiment_enabled ?? false);

  // Intervals and cleanup
  let timerInterval: number | null = null;
  let transcriptPoll: number | null = null;
  let meetingPoll: number | null = null;
  let unlisteners: (() => void)[] = [];

  function log(msg: string) {
    const t = new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
    activityLogs.update(l => [...l.slice(-99), `[${t}] ${msg}`]);
  }

  let starting = $state(false);

  async function start() {
    if (starting) return;
    starting = true;
    try {
      log("Starting recording...");
      clearAlerts();
      dismissAlertsByLevel("meeting");
      liveUtterances.set([]);
      liveSummary.set(null);

      // Update config before starting — must await so start_session reads the correct settings
      await invoke("update_config", {
        patch: {
          audio: {
            microphone: { enabled: micEnabled, device_index: selectedMicIndex },
            system_audio: { enabled: sysEnabled, app_bundle_id: selectedAppBundleId }
          }
        }
      });

      const result: any = await invoke("start_session");
      isRecording.set(true); isPaused.set(false);
      currentSessionId.set(result.id); sessionStartTime.set(Date.now());
      lastSessionIdStore.set(result.id);
      startTimer();
      startTranscriptPoll();
      log(`Recording started: ${result.id}${result.title ? " — " + result.title : ""}`);
    } catch (e) { log(`Error: ${e}`); }
    starting = false;
  }

  async function togglePause() {
    try {
      if ($isPaused) { await invoke("resume_session"); isPaused.set(false); log("Resumed"); }
      else { await invoke("pause_session"); isPaused.set(true); log("Paused"); }
    } catch (e) { log(`Error: ${e}`); }
  }

  async function stop() {
    try {
      const result: any = await invoke("stop_session");
      lastSessionIdStore.set(result.id);
      isRecording.set(false); isPaused.set(false);
      currentSessionId.set(null); sessionStartTime.set(null);
      stopTimer(); stopTranscriptPoll();
      vuMic = 0; vuSys = 0;
      dismissedMeetingApps.set(new Set());
      log(`Stopped: ${result.id} (${fmtDuration(result.duration_seconds)})`);
      // Load sentiment summary after session ends (short delay for DB writes to complete)
      setTimeout(async () => {
        try { sentimentData = await invoke("get_session_sentiment", { sessionId: result.id }); } catch (_) {}
      }, 3000);
    } catch (e) { log(`Error: ${e}`); }
  }

  function startTimer() {
    timerInterval = window.setInterval(() => {
      const st = get(sessionStartTime);
      if (st) timer = fmtTimer(Math.floor((Date.now() - st) / 1000));
    }, 250);
  }
  function stopTimer() { if (timerInterval) { clearInterval(timerInterval); timerInterval = null; } timer = ""; }

  function startTranscriptPoll() {
    transcriptPoll = window.setInterval(async () => {
      const sid = get(currentSessionId);
      if (!sid) return;
      try { liveUtterances.set(await invoke("get_transcript", { sessionId: sid })); } catch (_) {}
    }, 2000);
  }
  function stopTranscriptPoll() { if (transcriptPoll) { clearInterval(transcriptPoll); transcriptPoll = null; } }

  async function generateSummary() {
    const sid = $lastSessionIdStore || $currentSessionId;
    if (!sid) { log("No session to summarize"); return; }
    summaryLoading = true;
    log("Generating summary...");
    try {
      liveSummary.set(await invoke("summarize_session", { sessionId: sid }));
      log("Summary generated");
    } catch (e) { log(`Summary error: ${e}`); }
    summaryLoading = false;
  }

  async function checkMeetings() {
    if (get(isRecording)) { dismissAlertsByLevel("meeting"); return; }
    try {
      const meetings: any[] = await invoke("detect_meetings");
      if (meetings.length > 0) {
        const appKey = meetings.map(m => m.app_name).sort().join(",");
        if (!$dismissedMeetingApps.has(appKey)) {
          const names = meetings.map(m => m.app_name).join(", ");
          addAlert({
            level: "meeting",
            message: `Meeting detected: ${names}`,
            actions: [
              { label: "Record", handler: () => { dismissMeeting(appKey); start(); } },
              { label: "Dismiss", handler: () => dismissMeeting(appKey) },
            ],
            dismissable: false, // Use the explicit buttons instead
          });
        }
      } else {
        dismissAlertsByLevel("meeting");
        if ($dismissedMeetingApps.size > 0) dismissedMeetingApps.set(new Set());
      }
    } catch (_) {}
  }

  function dismissMeeting(appKey: string) {
    dismissedMeetingApps.update(s => { const n = new Set(s); n.add(appKey); return n; });
    dismissAlertsByLevel("meeting");
  }

  async function loadDevices() {
    try {
      const devices: any[] = await invoke("list_audio_devices");
      micDevices = devices.filter(d => d.device_type === "microphone" || d.device_type === "input");
      if (micDevices.length === 0) micDevices = devices;
      log(`${devices.length} audio device(s) found`);
    } catch (_) {}
    // Use SCK to get apps with bundle IDs (needed for per-app audio filtering).
    // Falls back to ps-based list if SCK isn't available.
    try {
      runningApps = await invoke("list_capturable_apps");
    } catch (_) {
      try { runningApps = await invoke("list_running_apps"); } catch (_) {}
    }
  }

  onMount(async () => {
    // Listen for real-time events from Rust EventBus
    // Payload is flat JSON (e.g. { source, db } for volume, { text, source } for transcript)
    const uv = await listen("gravai:volume", (e: any) => {
      const d = e.payload;
      if (!d?.source) return;
      const pct = Math.max(0, Math.min(100, ((d.db + 60) / 60) * 100));
      if (d.source === "microphone") vuMic = pct; else vuSys = pct;
    });
    unlisteners.push(uv);

    const ut = await listen("gravai:transcript", (e: any) => {
      const d = e.payload;
      if (d?.text && get(currentSessionId)) {
        liveUtterances.update(u => [...u, { ...d, timestamp: d.timestamp || new Date().toISOString() }]);
      }
    });
    unlisteners.push(ut);

    const us = await listen("gravai:silence-alert", (e: any) => {
      const d = e.payload;
      addAlert({
        level: "warning",
        message: d?.message || "No audio detected for 10+ seconds",
        dismissable: true,
      });
    });
    unlisteners.push(us);

    const ue = await listen("gravai:error", (e: any) => {
      const d = e.payload;
      const msg = d?.message || "Unknown error";
      log(`⚠️ ${msg}`);

      // Promote critical errors to visible alerts
      if (msg.includes("Transcription unavailable") || msg.includes("Model")) {
        addAlert({
          level: "error",
          message: msg,
          actions: [{ label: "Go to Models", handler: () => currentPage.set("models") }],
          dismissable: true,
        });
      } else if (msg.includes("closed") || msg.includes("Recording continues")) {
        addAlert({
          level: "warning",
          message: msg,
          dismissable: true,
        });
      }
    });
    unlisteners.push(ue);

    // Handle LLM transcript corrections arriving after utterances are inserted
    const uc = await listen("gravai:transcript-corrected", (e: any) => {
      const d = e.payload;
      const sid = get(currentSessionId);
      if (!d?.utterance_ids?.length || !sid || d.session_id !== sid) return;
      // Re-fetch the full transcript to pick up corrected_text values
      invoke("get_transcript", { sessionId: sid })
        .then((utts: any) => { if (Array.isArray(utts)) liveUtterances.set(utts); })
        .catch(() => {});
    });
    unlisteners.push(uc);

    await loadDevices();
    // Load export config + active preset/profile for status display
    try {
      const cfg: any = await invoke("get_config");
      exportAutoTranscript = cfg.export?.auto_export_transcript ?? false;
      exportAutoAudio = cfg.export?.auto_export_audio ?? false;
      exportRealtimeSave = cfg.export?.realtime_save ?? true;
      micEnabled = cfg.audio?.microphone?.enabled ?? true;
      sysEnabled = cfg.audio?.system_audio?.enabled ?? true;
      if (cfg.audio?.microphone?.device_index != null) selectedMicIndex = cfg.audio.microphone.device_index;
    } catch (_) {}
    try {
      const ps: any = await invoke("get_presets");
      if (ps.active_preset_id && ps.presets?.[ps.active_preset_id]) {
        activePreset = ps.presets[ps.active_preset_id];
      }
    } catch (_) {}
    try {
      const pr: any = await invoke("get_profiles");
      if (pr.active_profile_id && pr.profiles?.[pr.active_profile_id]) {
        activeProfile = pr.profiles[pr.active_profile_id];
      }
    } catch (_) {}
    checkMeetings();
    meetingPoll = window.setInterval(checkMeetings, 10000);
    // If a session was started externally (e.g. by automation) while this page
    // wasn't mounted, resume the timer and transcript poll.
    if (get(isRecording) && !timerInterval) {
      startTimer();
      startTranscriptPoll();
    }
    log("Gravai ready");
  });

  onDestroy(() => {
    stopTimer(); stopTranscriptPoll();
    if (meetingPoll) clearInterval(meetingPoll);
    for (const u of unlisteners) u();
  });

  // Split-screen resize
  let leftWidth = $state(380);

  function startResize(e: PointerEvent) {
    const startX = e.clientX;
    const startWidth = leftWidth;
    (e.target as HTMLElement).setPointerCapture(e.pointerId);

    function onMove(ev: PointerEvent) {
      leftWidth = Math.max(280, Math.min(620, startWidth + (ev.clientX - startX)));
    }
    function onUp(ev: PointerEvent) {
      (e.target as HTMLElement).releasePointerCapture(ev.pointerId);
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    }
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  }
</script>


<div class="recording-layout">
  <!-- Left panel: controls -->
  <div class="recording-left" style="flex: 0 0 {leftWidth}px">
    <div class="page-header">
      <h2>Recording</h2>
      <span class="timer">{timer}</span>
    </div>

    <!-- Active preset/profile indicators -->
    {#if activePreset || activeProfile}
      <div class="active-config-bar">
        {#if activePreset}
          <div class="config-pill-wrap">
            <div class="config-pill">
              <span class="config-pill-icon">🎛️</span>
              <span class="config-pill-label">Preset:</span>
              <span class="config-pill-value">{activePreset.name}</span>
              <button class="config-pill-edit" onclick={() => currentPage.set("presets")} title="Edit preset" aria-label="Edit preset">✎</button>
            </div>
            <div class="config-tooltip">
              <div class="config-tooltip-row">{activePreset.mic_enabled ? '🎤 Mic on' : '🎤 Mic off'} &middot; {activePreset.sys_enabled ? '💻 System on' : '💻 System off'}</div>
              <div class="config-tooltip-row">{activePreset.sample_rate/1000}kHz &middot; {activePreset.bit_depth}-bit &middot; {activePreset.channels === 1 ? 'Mono' : 'Stereo'}</div>
              <div class="config-tooltip-row">Format: {activePreset.export_format}</div>
              {#if activePreset.output_folder}<div class="config-tooltip-row">📁 {activePreset.output_folder}</div>{/if}
              {#if activeProfile}
                <div class="config-tooltip-divider"></div>
                <div class="config-tooltip-row">🗣️ Model: Whisper {activeProfile.transcription_model || 'medium'}</div>
              {/if}
            </div>
          </div>
        {/if}
        {#if activeProfile}
          <div class="config-pill-wrap">
            <div class="config-pill">
              <span class="config-pill-icon">👤</span>
              <span class="config-pill-label">Profile:</span>
              <span class="config-pill-value">{activeProfile.name}</span>
              <button class="config-pill-edit" onclick={() => currentPage.set("profiles")} title="Edit profile" aria-label="Edit profile">✎</button>
            </div>
            <div class="config-tooltip">
              <div class="config-tooltip-row">🗣️ Whisper {activeProfile.transcription_model || 'medium'} &middot; {activeProfile.transcription_language || 'en'}</div>
              <div class="config-tooltip-row">🤖 {activeProfile.llm_provider || 'ollama'} ({activeProfile.llm_model || 'gemma3:4b'})</div>
              <div class="config-tooltip-row">👥 Diarization: {activeProfile.diarization_enabled ? 'on' : 'off'} &middot; Echo: {activeProfile.echo_suppression_enabled !== false ? 'on' : 'off'}</div>
              {#if activeProfile.auto_export_transcript}<div class="config-tooltip-row">📝 Auto-export transcript</div>{/if}
              {#if activeProfile.realtime_save !== false}<div class="config-tooltip-row">💾 Real-time save</div>{/if}
            </div>
          </div>
        {/if}
      </div>
    {/if}

    <!-- Transport -->
    <div class="transport">
      <button class="transport-btn record" class:active={$isRecording && !$isPaused} disabled={$isRecording || starting} onclick={start} title="Record">{starting ? "⏳" : "⏺"}</button>
      <button class="transport-btn pause" disabled={!$isRecording} onclick={togglePause} title={$isPaused ? "Resume" : "Pause"}>{$isPaused ? "▶" : "⏸"}</button>
      <button class="transport-btn stop" disabled={!$isRecording && !starting} onclick={stop} title="Stop">⏹</button>
      <span class="status-badge" class:recording={$isRecording && !$isPaused} class:paused={$isRecording && $isPaused} class:idle={!$isRecording && !starting} class:starting={starting}>
        {starting ? "Starting..." : $isRecording ? ($isPaused ? "Paused" : "Recording") : "Idle"}
      </span>
    </div>

    <!-- Export status indicator -->
    {#if exportAutoTranscript || exportAutoAudio || exportRealtimeSave}
      <div class="export-indicator">
        {#if exportRealtimeSave}<span class="export-tag save">💾 Auto-save</span>{/if}
        {#if exportAutoTranscript}<span class="export-tag transcript">📝 Auto-export transcript</span>{/if}
        {#if exportAutoAudio}<span class="export-tag audio">🔊 Auto-export audio</span>{/if}
      </div>
    {/if}

    <!-- Audio Sources -->
    <details class="card collapsible" open>
      <summary class="card-header">Audio Sources</summary>
      <div class="source-grid">
        <!-- Mic row -->
        <div class="source-row">
          <div class="source-row-top">
            <label class="source-toggle"><input type="checkbox" class="toggle" bind:checked={micEnabled} /> 🎤 Microphone</label>
            <div class="source-meter"><div class="vu-meter"><div class="vu-fill" style="width: {vuMic}%"></div></div></div>
          </div>
          <div class="source-row-bottom">
            <div class="source-device">
              <select class="select select-sm" bind:value={selectedMicIndex}>
                <option value={-1}>Default mic</option>
                {#each micDevices as d}
                  <option value={d.index}>{d.name}</option>
                {/each}
              </select>
            </div>
            <div class="source-volume">
              <input type="range" class="slider" min="0" max="200" bind:value={micVolume} />
              <span class="slider-value">{micVolume}%</span>
            </div>
          </div>
        </div>
        <!-- System audio row -->
        <div class="source-row">
          <div class="source-row-top">
            <label class="source-toggle"><input type="checkbox" class="toggle" bind:checked={sysEnabled} /> 💻 System Audio</label>
            <div class="source-meter"><div class="vu-meter"><div class="vu-fill" style="width: {vuSys}%"></div></div></div>
          </div>
          <div class="source-row-bottom">
            <div class="source-device">
              <AppPicker
                apps={runningApps}
                selected={selectedAppBundleId}
                onselect={(v) => selectedAppBundleId = v}
              />
            </div>
            <div class="source-volume">
              <input type="range" class="slider" min="0" max="200" bind:value={sysVolume} />
              <span class="slider-value">{sysVolume}%</span>
            </div>
          </div>
        </div>
      </div>
    </details>

    <!-- Summary (visible when we have transcript OR after stop) -->
    {#if $liveUtterances.length > 0 || $lastSessionIdStore}
      <details class="card collapsible" open>
        <summary class="card-header">
          Summary
          <button class="btn btn-xs btn-accent" onclick={(e) => { e.stopPropagation(); generateSummary(); }} disabled={summaryLoading}>
            {summaryLoading ? "⏳ Generating..." : "Generate Summary"}
          </button>
        </summary>
        {#if $liveSummary}
          <div class="summary-content">
            <h4>TL;DR</h4>
            <p>{$liveSummary.tldr}</p>
            {#if $liveSummary.key_decisions?.length}
              <h4>Key Decisions</h4>
              <ul>{#each $liveSummary.key_decisions as d}<li>{d}</li>{/each}</ul>
            {/if}
            {#if $liveSummary.action_items?.length}
              <h4>Action Items</h4>
              <ul>{#each $liveSummary.action_items as a}<li>{a.description} {#if a.owner}<span class="action-owner">@{a.owner}</span>{/if}</li>{/each}</ul>
            {/if}
            {#if $liveSummary.open_questions?.length}
              <h4>Open Questions</h4>
              <ul>{#each $liveSummary.open_questions as q}<li>{q}</li>{/each}</ul>
            {/if}
          </div>
        {:else}
          <div class="empty-state">Click "Generate Summary" to create a meeting brief.</div>
        {/if}
      </details>
    {/if}

    <!-- Sentiment Summary (shown after session stops, system audio only) -->
    {#if sentimentData?.speakers?.length > 0}
      <details class="card collapsible" open>
        <summary class="card-header">Sentiment — Participants</summary>
        <div class="sentiment-summary">
          {#each sentimentData.speakers as sp}
            <div class="sentiment-speaker">
              <span class="sentiment-speaker-name">{sp.speaker}</span>
              <span class="sentiment-dominant">{sp.dominant_emotion}</span>
              <span class="sentiment-count">{sp.utterance_count} utterances</span>
            </div>
          {/each}
        </div>
      </details>
    {/if}

    <!-- Activity Log -->
    <details class="card collapsible" open>
      <summary class="card-header">Activity Log</summary>
      <div class="log-panel">
        {#each $activityLogs as line}<div>{line}</div>{/each}
      </div>
    </details>
  </div>

  <!-- Drag resizer -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="recording-resizer" onpointerdown={startResize}></div>

  <!-- Right panel: live transcript -->
  <div class="recording-right">
    <div class="transcript-panel-header">
      <span class="transcript-panel-title">Live Transcript</span>
      <button type="button" class="header-toggle-btn" onclick={() => autoScrollEnabled.set(!$autoScrollEnabled)}>
        {$autoScrollEnabled ? "☑" : "☐"} Auto-scroll
      </button>
    </div>
    <div class="transcript-panel-body">
      <TranscriptView utterances={$liveUtterances} autoScroll={$autoScrollEnabled} {showEmotions} sessionId={$currentSessionId ?? $lastSessionIdStore} />
    </div>
  </div>
</div>

<style>
  /* Override content area — remove all padding and scroll so panels fill the full area */
  :global(.content:has(.recording-layout)) {
    overflow: hidden;
    padding: 0;
    gap: 0;
  }

  .recording-layout {
    display: flex;
    flex: 1;
    gap: 0;
    min-height: 0;
    height: 100%;
  }
  .recording-left {
    display: flex;
    flex-direction: column;
    gap: 12px;
    overflow-y: auto;
    padding: 16px 8px 16px 20px;
    min-width: 280px;
    flex-shrink: 0;
  }
  .recording-resizer {
    width: 6px;
    cursor: col-resize;
    background: transparent;
    flex-shrink: 0;
    border-radius: 3px;
    transition: background 0.15s;
    margin: 0 2px;
  }
  .recording-resizer:hover {
    background: var(--border);
  }
  .recording-right {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .transcript-panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 14px 10px;
    border-bottom: 1px solid var(--border-subtle);
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
    flex-shrink: 0;
  }
  .transcript-panel-title {
    text-transform: uppercase;
    letter-spacing: 0.5px;
    font-size: 11px;
  }
  .transcript-panel-body {
    flex: 1;
    overflow: hidden;
    min-height: 0;
  }
  .sentiment-summary { padding: 8px 14px; display: flex; flex-direction: column; gap: 6px; }
  .sentiment-speaker {
    display: flex; align-items: center; gap: 8px; font-size: 12px;
  }
  .sentiment-speaker-name { font-weight: 600; color: var(--text-primary); min-width: 70px; }
  .sentiment-dominant {
    background: rgba(255,255,255,0.06); padding: 2px 8px; border-radius: 4px;
    font-size: 11px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.3px;
    color: var(--accent);
  }
  .sentiment-count { font-size: 10px; color: var(--text-tertiary); margin-left: auto; }
</style>
