<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke, listen, fmtTimer, fmtDuration } from "../lib/tauri";
  import { isRecording, isPaused, currentSessionId, sessionStartTime, autoScrollEnabled } from "../lib/store";
  import TranscriptView from "../components/TranscriptView.svelte";
  import AppPicker from "../components/AppPicker.svelte";

  let timer = $state("");
  let micEnabled = $state(true);
  let sysEnabled = $state(true);
  let micVolume = $state(100);
  let sysVolume = $state(100);
  let vuMic = $state(0);
  let vuSys = $state(0);
  let utterances = $state<any[]>([]);
  let logs = $state<string[]>([]);
  let meetingBanner = $state<string | null>(null);
  let dismissedApps = $state<Set<string>>(new Set());  // Track dismissed app combinations
  let summary = $state<any>(null);
  let summaryLoading = $state(false);
  let lastSessionId = $state<string | null>(null);

  // Device selection
  let micDevices = $state<any[]>([]);
  let selectedMicIndex = $state(-1);
  let runningApps = $state<any[]>([]);
  let selectedAppBundleId = $state("");

  // Intervals and cleanup
  let timerInterval: number | null = null;
  let transcriptPoll: number | null = null;
  let meetingPoll: number | null = null;
  let unlisteners: (() => void)[] = [];

  function log(msg: string) {
    const t = new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
    logs = [...logs.slice(-99), `[${t}] ${msg}`];
  }

  async function start() {
    try {
      log("Starting recording...");
      // Update config with source selections (fire-and-forget for speed)
      invoke("update_config", {
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
      lastSessionId = result.id;
      utterances = [];
      summary = null;
      meetingBanner = null; // Hide banner while recording
      startTimer();
      startTranscriptPoll();
      log(`Recording started: ${result.id}${result.title ? " — " + result.title : ""}`);
    } catch (e) { log(`Error: ${e}`); }
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
      lastSessionId = result.id; // Keep for summary generation
      isRecording.set(false); isPaused.set(false);
      currentSessionId.set(null); sessionStartTime.set(null);
      stopTimer(); stopTranscriptPoll();
      vuMic = 0; vuSys = 0;
      dismissedApps = new Set(); // Reset dismissals after recording ends
      log(`Stopped: ${result.id} (${fmtDuration(result.duration_seconds)})`);
    } catch (e) { log(`Error: ${e}`); }
  }

  function startTimer() {
    timerInterval = window.setInterval(() => {
      const st = $sessionStartTime;
      if (st) timer = fmtTimer(Math.floor((Date.now() - st) / 1000));
    }, 250);
  }
  function stopTimer() { if (timerInterval) { clearInterval(timerInterval); timerInterval = null; } timer = ""; }

  function startTranscriptPoll() {
    transcriptPoll = window.setInterval(async () => {
      const sid = $currentSessionId;
      if (!sid) return;
      try { utterances = await invoke("get_transcript", { sessionId: sid }); } catch (_) {}
    }, 2000);
  }
  function stopTranscriptPoll() { if (transcriptPoll) { clearInterval(transcriptPoll); transcriptPoll = null; } }

  async function generateSummary() {
    // Use lastSessionId (persists after stop) or currentSessionId
    const sid = lastSessionId || $currentSessionId;
    if (!sid) { log("No session to summarize"); return; }
    summaryLoading = true;
    log("Generating summary...");
    try {
      summary = await invoke("summarize_session", { sessionId: sid });
      log("Summary generated");
    } catch (e) { log(`Summary error: ${e}`); }
    summaryLoading = false;
  }

  async function checkMeetings() {
    if ($isRecording) { meetingBanner = null; return; }
    try {
      const meetings: any[] = await invoke("detect_meetings");
      if (meetings.length > 0) {
        // Create a key from the detected app names to track dismissals
        const appKey = meetings.map(m => m.app_name).sort().join(",");
        if (dismissedApps.has(appKey)) {
          meetingBanner = null; // Already dismissed this exact combination
        } else {
          meetingBanner = `Meeting detected: ${meetings.map(m => m.app_name).join(", ")}`;
        }
      } else {
        meetingBanner = null;
        // If no meetings are running, clear dismissed set (apps changed)
        if (dismissedApps.size > 0) dismissedApps = new Set();
      }
    } catch (_) {}
  }

  function dismissBanner() {
    // Track which app combination was dismissed so it doesn't reappear
    if (meetingBanner) {
      const appNames = meetingBanner.replace("Meeting detected: ", "");
      dismissedApps = new Set([...dismissedApps, appNames.split(", ").sort().join(",")]);
    }
    meetingBanner = null;
  }

  async function loadDevices() {
    try {
      const devices: any[] = await invoke("list_audio_devices");
      micDevices = devices.filter(d => d.device_type === "microphone" || d.device_type === "input");
      // If no type filtering worked, show all
      if (micDevices.length === 0) micDevices = devices;
      log(`${devices.length} audio device(s) found`);
    } catch (_) {}
    try {
      runningApps = await invoke("list_running_apps");
    } catch (_) {}
  }

  onMount(async () => {
    const uv = await listen("gravai:volume", (e: any) => {
      const d = e.payload?.data || e.payload;
      if (!d?.source) return;
      const pct = Math.max(0, Math.min(100, ((d.db + 60) / 60) * 100));
      if (d.source === "microphone") vuMic = pct; else vuSys = pct;
    });
    unlisteners.push(uv);

    const ut = await listen("gravai:transcript", (e: any) => {
      const d = e.payload?.data || e.payload;
      if (d?.text && $currentSessionId) {
        utterances = [...utterances, { ...d, timestamp: d.timestamp || new Date().toISOString() }];
      }
    });
    unlisteners.push(ut);

    const ue = await listen("gravai:error", (e: any) => {
      const d = e.payload?.data || e.payload;
      log(`⚠️ ${d?.message || "Error"}`);
    });
    unlisteners.push(ue);

    await loadDevices();
    checkMeetings();
    meetingPoll = window.setInterval(checkMeetings, 10000);
    log("Gravai ready");
  });

  onDestroy(() => {
    stopTimer(); stopTranscriptPoll();
    if (meetingPoll) clearInterval(meetingPoll);
    for (const u of unlisteners) u();
  });
</script>

{#if meetingBanner}
  <div class="banner banner-accent">
    <span class="banner-text">{meetingBanner}</span>
    <div class="banner-actions">
      <button class="btn btn-xs btn-accent" onclick={() => { dismissBanner(); start(); }}>Record</button>
      <button class="btn btn-xs btn-ghost" onclick={dismissBanner}>Dismiss</button>
    </div>
  </div>
{/if}

<div class="page-header">
  <h2>Recording</h2>
  <span class="timer">{timer}</span>
</div>

<!-- Transport -->
<div class="transport">
  <button class="transport-btn record" class:active={$isRecording && !$isPaused} disabled={$isRecording} onclick={start} title="Record">⏺</button>
  <button class="transport-btn pause" disabled={!$isRecording} onclick={togglePause} title={$isPaused ? "Resume" : "Pause"}>{$isPaused ? "▶" : "⏸"}</button>
  <button class="transport-btn stop" disabled={!$isRecording} onclick={stop} title="Stop">⏹</button>
  <span class="status-badge" class:recording={$isRecording && !$isPaused} class:paused={$isRecording && $isPaused} class:idle={!$isRecording}>
    {$isRecording ? ($isPaused ? "Paused" : "Recording") : "Idle"}
  </span>
</div>

<!-- Audio Sources -->
<details class="card collapsible" open>
  <summary class="card-header">Audio Sources</summary>
  <div class="source-grid">
    <!-- Mic row -->
    <div class="source-row">
      <label class="source-toggle"><input type="checkbox" class="toggle" bind:checked={micEnabled} /> 🎤 Microphone</label>
      <div class="source-device">
        <select class="select select-sm" bind:value={selectedMicIndex}>
          <option value={-1}>Default mic</option>
          {#each micDevices as d}
            <option value={d.index}>{d.name}</option>
          {/each}
        </select>
      </div>
      <div class="source-meter"><div class="vu-meter"><div class="vu-fill" style="width: {vuMic}%"></div></div></div>
      <div class="source-volume">
        <input type="range" class="slider" min="0" max="200" bind:value={micVolume} />
        <span class="slider-value">{micVolume}%</span>
      </div>
    </div>
    <!-- System audio row -->
    <div class="source-row">
      <label class="source-toggle"><input type="checkbox" class="toggle" bind:checked={sysEnabled} /> 💻 System Audio</label>
      <div class="source-device">
        <AppPicker
          apps={runningApps}
          selected={selectedAppBundleId}
          onselect={(v) => selectedAppBundleId = v}
        />
      </div>
      <div class="source-meter"><div class="vu-meter"><div class="vu-fill" style="width: {vuSys}%"></div></div></div>
      <div class="source-volume">
        <input type="range" class="slider" min="0" max="200" bind:value={sysVolume} />
        <span class="slider-value">{sysVolume}%</span>
      </div>
    </div>
  </div>
</details>

<!-- Live Transcript -->
<details class="card collapsible" open>
  <summary class="card-header">
    Live Transcript
    <span class="header-toggle">
      <input type="checkbox" checked={$autoScrollEnabled} onclick={(e) => e.stopPropagation()} onchange={(e) => autoScrollEnabled.set((e.target as HTMLInputElement).checked)} />
      <span onclick={(e) => e.stopPropagation()}>Auto-scroll</span>
    </span>
  </summary>
  <TranscriptView {utterances} autoScroll={$autoScrollEnabled} />
</details>

<!-- Summary (visible when we have transcript OR after stop) -->
{#if utterances.length > 0 || lastSessionId}
  <details class="card collapsible" open>
    <summary class="card-header">
      Summary
      <button class="btn btn-xs btn-accent" onclick={(e) => { e.stopPropagation(); generateSummary(); }} disabled={summaryLoading}>
        {summaryLoading ? "⏳ Generating..." : "Generate Summary"}
      </button>
    </summary>
    {#if summary}
      <div class="summary-content">
        <h4>TL;DR</h4>
        <p>{summary.tldr}</p>
        {#if summary.key_decisions?.length}
          <h4>Key Decisions</h4>
          <ul>{#each summary.key_decisions as d}<li>{d}</li>{/each}</ul>
        {/if}
        {#if summary.action_items?.length}
          <h4>Action Items</h4>
          <ul>{#each summary.action_items as a}<li>{a.description} {#if a.owner}<span class="action-owner">@{a.owner}</span>{/if}</li>{/each}</ul>
        {/if}
        {#if summary.open_questions?.length}
          <h4>Open Questions</h4>
          <ul>{#each summary.open_questions as q}<li>{q}</li>{/each}</ul>
        {/if}
      </div>
    {:else}
      <div class="empty-state">Click "Generate Summary" to create a meeting brief.</div>
    {/if}
  </details>
{/if}

<!-- Activity Log -->
<details class="card collapsible" open>
  <summary class="card-header">Activity Log</summary>
  <div class="log-panel">
    {#each logs as line}<div>{line}</div>{/each}
  </div>
</details>
