<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke, listen, fmtTimer, fmtDuration, copyToClipboard } from "../lib/tauri";
  import { get } from "svelte/store";
  import { sourceWarnings, isRecording, isPaused, currentSessionId, sessionStartTime, autoScrollEnabled, liveUtterances, lastSessionId as lastSessionIdStore, activityLogs, liveSummary, bookmarkCount, dismissedMeetingApps, addAlert, dismissAlertsByLevel, clearAlerts, navigate, finishedSessionId, pendingArchiveSessionId } from "../lib/store";
  import { config, loadConfig, setValue } from "../lib/config";
  import RunBar from "../components/RunBar.svelte";
  import TranscriptView from "../components/TranscriptView.svelte";
  import AppPicker from "../components/AppPicker.svelte";
  import Icon from "../components/Icon.svelte";
  import LlmStatusBanner from "../components/LlmStatusBanner.svelte";

  let timer = $state("");
  let micEnabled = $state(true);
  let sysEnabled = $state(true);
  let vuMic = $state(0);
  let vuSys = $state(0);
  let summaryLoading = $state(false);
  let summaryError = $state<string | null>(null);
  let sentimentData = $state<any>(null);
  // Becomes true once the one delayed post-stop transcript refetch has run
  // and found nothing — distinct from "haven't checked yet" (null-ish via
  // liveUtterances still being populated from the live poll).
  let emptyTranscriptAfterStop = $state(false);
  // True for the ~2s between stop() and finalizeAfterStop() resolving, so the
  // finished view can show a neutral "checking" message instead of briefly
  // flashing "Summary unavailable" before we know the transcript is there.
  let finalizing = $state(false);
  // Timestamps of the last non-zero VU level per source, used to show a
  // "No sound from <source>" hint while recording (see gravai:volume below).
  let lastMicSoundAt = $state<number | null>(null);
  let lastSysSoundAt = $state<number | null>(null);

  let starting = $state(false);
  let sessionTitle = $state<string>("");
  let editingTitle = $state(false);
  let titleEditValue = $state("");

  // Bookmarks for the just-finished session, shown as "mm:ss  note" in the
  // finished view. The current recording's running count lives in the
  // shared `bookmarkCount` store (see gravai:bookmark below).
  let sessionBookmarks = $state<any[]>([]);
  // Key-sequence label for the bookmark shortcut, read from the shared
  // shortcut store so the tooltip reflects the user's actual binding
  // (recording.bookmark — handled window-side in StatusBar.svelte since
  // it's a non-global shortcut).
  let bookmarkShortcutLabel = $state("");
  // The "Add a note…" prompt shown right after a bookmark is created
  // (button or shortcut) — see the gravai:bookmark listener below.
  let pendingBookmarkNote = $state<{ id: number; offsetMs: number } | null>(null);
  let bookmarkNoteValue = $state("");
  let bookmarkNoteTimeout: number | null = null;

  // Device selection. Empty name means the system default input.
  let micDevices = $state<any[]>([]);
  let selectedMicName = $state("");
  let knownMicNames = "";
  let devicePoll: number | null = null;
  let runningApps = $state<any[]>([]);
  let selectedAppBundleId = $state("");

  // The live config: source switches here and in Settings are the same values.
  let runConfig = $derived($config);
  let exportAutoTranscript = $derived(runConfig?.export?.auto_export_transcript ?? false);
  let exportRealtimeSave = $derived(runConfig?.export?.realtime_save ?? true);
  let showEmotions = $derived(runConfig?.features?.sentiment?.enabled ?? false);
  let autoSummaryOnStop = $derived(runConfig?.features?.auto_summary_on_stop ?? true);
  // The session just stopped is shown as a "finished object" instead of an
  // empty idle page until the user explicitly leaves it (New recording) or
  // starts a new one. Navigating to another page and back does not clear it.
  let finishedView = $derived(!!$finishedSessionId && !$isRecording && !starting);
  // "Is sound coming in?" hint: shown as soon as the startup source check
  // flags a source, and later when an enabled source has been silent for
  // more than 10s while recording.
  let noSoundMic = $derived.by(() => {
    void timer; // re-evaluate on the recording tick
    return $isRecording && micEnabled && (!!$sourceWarnings.microphone || (lastMicSoundAt != null && Date.now() - lastMicSoundAt > 10_000));
  });
  let noSoundSys = $derived.by(() => {
    void timer;
    return $isRecording && sysEnabled && (!!$sourceWarnings.system_audio || (lastSysSoundAt != null && Date.now() - lastSysSoundAt > 10_000));
  });
  $effect(() => {
    if (!runConfig) return;
    micEnabled = runConfig.audio?.microphone?.enabled ?? true;
    sysEnabled = runConfig.audio?.system_audio?.enabled ?? true;
    selectedAppBundleId = runConfig.audio?.system_audio?.app_bundle_id ?? "";
  });


  // Intervals and cleanup
  let timerInterval: number | null = null;
  let transcriptPoll: number | null = null;
  let meetingPoll: number | null = null;
  let unlisteners: (() => void)[] = [];
  // Tracks the highest utterance id received so far; used for incremental poll.
  let lastUtteranceId = 0;

  function log(msg: string) {
    const t = new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
    activityLogs.update(l => [...l.slice(-99), `[${t}] ${msg}`]);
  }

  async function start() {
    if (starting) return;
    starting = true;
    // Leave the just-finished session behind — this is a genuinely new
    // recording, not a return visit to the old one.
    finishedSessionId.set(null);
    sessionTitle = "";
    sentimentData = null;
    summaryError = null;
    emptyTranscriptAfterStop = false;
    finalizing = false;
    lastMicSoundAt = Date.now();
    lastSysSoundAt = Date.now();
    try {
      log("Starting recording...");
      clearAlerts();
      dismissAlertsByLevel("meeting");
      liveUtterances.set([]);
      liveSummary.set(null);
      bookmarkCount.set(0);
      lastUtteranceId = 0;

      // Update config before starting — must await so start_session reads the correct settings
      await invoke("update_config", {
        patch: {
          audio: {
            microphone: { enabled: micEnabled, device_name: selectedMicName, device_index: -1 },
            system_audio: { enabled: sysEnabled, app_bundle_id: selectedAppBundleId }
          }
        }
      });

      const result: any = await invoke("start_session");
      isRecording.set(true); isPaused.set(false);
      currentSessionId.set(result.id); sessionStartTime.set(Date.now());
      lastSessionIdStore.set(result.id);
      // Only set title from start_session if it returned one AND we don't
      // already have one (calendar EventKit may have set it via progress event
      // before start_session returned)
      if (result.title) sessionTitle = result.title;
      else if (!sessionTitle) sessionTitle = "";
      startTimer();
      startTranscriptPoll();
      log(`Recording started: ${result.id}${result.title ? " — " + result.title : ""}`);
    } catch (e) { log(`Error: ${e}`); }
    starting = false;
  }

  async function addBookmark() {
    if (!$isRecording) return;
    try {
      const result: any = await invoke("add_bookmark", { note: null });
      // Don't increment here — the gravai:bookmark event listener handles the count
      // (it also opens the "Add a note…" prompt below, for both this button
      // and the recording.bookmark keyboard shortcut).
      const secs = Math.floor(result.offset_ms / 1000);
      const m = String(Math.floor(secs / 60)).padStart(2, "0");
      const s = String(secs % 60).padStart(2, "0");
      log(`Bookmark added at ${m}:${s}`);
    } catch (e) { log(`Bookmark error: ${e}`); }
  }

  function fmtOffsetMs(ms: number): string {
    const secs = Math.floor(ms / 1000);
    const m = String(Math.floor(secs / 60)).padStart(2, "0");
    const s = String(secs % 60).padStart(2, "0");
    return `${m}:${s}`;
  }

  function prettyShortcut(seq: string): string {
    if (!seq) return "";
    const sym: Record<string, string> = { CmdOrCtrl: "⌘", Alt: "⌥", Shift: "⇧", Ctrl: "⌃", Space: "Space" };
    return seq.split("+").map(p => sym[p] ?? p).join("");
  }

  async function saveBookmarkNote() {
    const note = bookmarkNoteValue.trim();
    const id = pendingBookmarkNote?.id;
    dismissBookmarkNote();
    if (id == null) return;
    try { await invoke("update_bookmark_note", { bookmarkId: id, note: note || null }); } catch (_) {}
  }

  function dismissBookmarkNote() {
    if (bookmarkNoteTimeout) { clearTimeout(bookmarkNoteTimeout); bookmarkNoteTimeout = null; }
    pendingBookmarkNote = null;
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
      const sid = result.id;
      lastSessionIdStore.set(sid);
      // Show this session as a finished object instead of dropping back to
      // an empty Record page. The title is NOT cleared — it stays as-is
      // (and stays editable via the existing rename flow) until the user
      // explicitly leaves via "New recording" or starts a new one.
      finishedSessionId.set(sid);
      emptyTranscriptAfterStop = false;
      finalizing = true;
      isRecording.set(false); isPaused.set(false);
      currentSessionId.set(null); sessionStartTime.set(null);
      stopTimer(); stopTranscriptPoll();
      vuMic = 0; vuSys = 0;
      dismissedMeetingApps.set(new Set());
      dismissBookmarkNote();
      log(`Stopped: ${sid} (${fmtDuration(result.duration_seconds)})`);
      // Bookmarks are committed immediately when added, so no delay needed
      // (unlike sentiment/transcript, which can lag DB writes briefly).
      try {
        const bms: any[] = await invoke("list_bookmarks", { sessionId: sid });
        if (get(finishedSessionId) === sid) sessionBookmarks = bms;
      } catch (_) {}
      // Load sentiment summary after session ends (short delay for DB writes to complete)
      setTimeout(async () => {
        try {
          const s = await invoke("get_session_sentiment", { sessionId: sid });
          if (get(finishedSessionId) === sid) sentimentData = s;
        } catch (_) {}
      }, 3000);
      // One delayed refetch (~2s) so late-finalised utterances (e.g. the
      // tail end of a VAD segment still being transcribed) show up before
      // we decide whether there's anything to summarize.
      setTimeout(() => finalizeAfterStop(sid), 2000);
    } catch (e) { log(`Error: ${e}`); }
  }

  /** Runs once, ~2s after stop(): re-fetches the transcript from the DB,
   * then starts the summary automatically if conditions are met. Never
   * touches state for a session the user has since left (New recording /
   * a newer recording started). */
  async function finalizeAfterStop(sid: string) {
    let utts: any[] = [];
    try {
      utts = await invoke("get_transcript", { sessionId: sid });
    } catch (_) {
      if (get(finishedSessionId) === sid) finalizing = false;
      return;
    }
    // The user left this session (New recording, or started another one)
    // before the refetch landed — don't resurrect it.
    if (get(finishedSessionId) !== sid) return;
    liveUtterances.set(utts);
    if (utts.length > 0) lastUtteranceId = Math.max(0, ...utts.map((u: any) => u.id));
    finalizing = false;
    if (utts.length === 0) {
      emptyTranscriptAfterStop = true;
      log("No speech was captured");
      return;
    }
    // Auto-summary: only if the config flag is on and nothing is already
    // loading/loaded for this session (a manual click during recording
    // already produced one, or a Retry is in flight).
    if (autoSummaryOnStop && !summaryLoading && !get(liveSummary)) {
      await generateSummary(sid);
    }
  }

  /** Leaves the finished session behind, returning Record to idle. */
  function newRecording() {
    finishedSessionId.set(null);
    sessionTitle = "";
    liveUtterances.set([]);
    liveSummary.set(null);
    bookmarkCount.set(0);
    sentimentData = null;
    summaryError = null;
    emptyTranscriptAfterStop = false;
    finalizing = false;
    sessionBookmarks = [];
    dismissBookmarkNote();
  }

  let copiedPath = $state(false);
  let copyPathError = $state<string | null>(null);

  async function copyMarkdownPath() {
    const sid = get(finishedSessionId);
    if (!sid) return;
    copyPathError = null;
    try {
      const path: string = await invoke("get_session_markdown_path", { sessionId: sid });
      await copyToClipboard(path);
      copiedPath = true;
      setTimeout(() => { copiedPath = false; }, 1500);
    } catch (e) {
      copyPathError = String(e);
      setTimeout(() => { copyPathError = null; }, 3000);
    }
  }

  function openInArchive() {
    const sid = get(finishedSessionId);
    if (!sid) return;
    pendingArchiveSessionId.set(sid);
    navigate("archive");
  }

  function startTimer() {
    timerInterval = window.setInterval(() => {
      const st = get(sessionStartTime);
      if (st) timer = fmtTimer(Math.floor((Date.now() - st) / 1000));
    }, 250);
  }
  function stopTimer() { if (timerInterval) { clearInterval(timerInterval); timerInterval = null; } timer = ""; }

  let pollInFlight = false;
  async function pollTranscript() {
    if (pollInFlight) return;
    const sid = get(currentSessionId);
    if (!sid) return;
    pollInFlight = true;
    try {
      const newUtts: any[] = await invoke("get_transcript_since", { sessionId: sid, afterId: lastUtteranceId });
      if (newUtts.length > 0) {
        lastUtteranceId = newUtts[newUtts.length - 1].id;
        liveUtterances.update(current => {
          const existingIds = new Set(current.map((u: any) => u.id));
          const fresh = newUtts.filter((u: any) => !existingIds.has(u.id));
          return fresh.length > 0 ? [...current, ...fresh] : current;
        });
      }
    } catch (_) {}
    pollInFlight = false;
  }
  function startTranscriptPoll() {
    transcriptPoll = window.setInterval(pollTranscript, 2000);
  }
  function stopTranscriptPoll() { if (transcriptPoll) { clearInterval(transcriptPoll); transcriptPoll = null; } }

  async function generateSummary(sid?: string) {
    const targetId = sid ?? ($lastSessionIdStore || $currentSessionId);
    if (!targetId) { log("No session to summarize"); return; }
    if (summaryLoading) return; // guard against double runs (button + auto-trigger)
    summaryLoading = true;
    summaryError = null;
    log("Generating summary...");
    // Still relevant if the user hasn't moved on to a different session
    // (started a new recording, or left this one) by the time this resolves.
    const stillRelevant = () => get(currentSessionId) === targetId || get(finishedSessionId) === targetId;
    try {
      const result = await invoke("summarize_session", { sessionId: targetId });
      if (stillRelevant()) {
        liveSummary.set(result);
        log("Summary generated");
      } else {
        log(`Summary for ${targetId} arrived after switching sessions — discarded`);
      }
    } catch (e: any) {
      const msg = typeof e === "string" ? e : (e?.message ?? "Summary failed");
      if (stillRelevant()) summaryError = msg;
      log(`Summary error: ${msg}`);
    }
    summaryLoading = false;
  }

  async function checkMeetings() {
    if (get(isRecording)) { dismissAlertsByLevel("meeting"); return; }
    try {
      const cfg: any = await invoke("get_config");
      if (cfg?.features?.meeting_detection?.enabled === false) {
        dismissAlertsByLevel("meeting");
        return;
      }
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

  async function applyMicChoice() {
    const name = selectedMicName;
    try {
      await invoke("update_config", {
        patch: {
          audio: {
            microphone: { enabled: true, device_name: name, device_index: -1 },
          },
        },
      });
      micEnabled = true;
      if (get(isRecording)) {
        const label: string = await invoke("switch_microphone", { deviceName: name });
        if (name) selectedMicName = label || name;
        log(`Microphone listening: ${label || "default"}`);
      } else {
        log(`Microphone set: ${name || "default"}`);
      }
    } catch (e) {
      log(`Microphone switch failed: ${e}`);
    }
  }

  async function loadDevices() {
    try {
      const devices: any[] = await invoke("list_audio_devices");
      const mics = devices.filter(d => d.device_type === "microphone" || d.device_type === "input");
      const next = mics.length === 0 ? devices : mics;
      const key = next.map(d => d.name).join("\n");
      if (key !== knownMicNames) {
        if (knownMicNames && selectedMicName && !next.some(d => d.name === selectedMicName)) {
          log(`Microphone “${selectedMicName}” is no longer connected`);
        }
        knownMicNames = key;
      }
      micDevices = next;
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
      // Same -50 dB threshold as the backend source check; noise floor doesn't
      // count as "sound coming in".
      const hasSound = d.db > -50;
      if (d.source === "microphone") { vuMic = pct; if (hasSound) lastMicSoundAt = Date.now(); }
      else { vuSys = pct; if (hasSound) lastSysSoundAt = Date.now(); }
    });
    unlisteners.push(uv);

    const ut = await listen("gravai:transcript", (_e: any) => {
      // Don't add directly — trigger an immediate poll so the DB is the single
      // source of truth and we never get dual-write duplicates.
      if (get(currentSessionId)) pollTranscript();
    });
    unlisteners.push(ut);

    const ue = await listen("gravai:error", (e: any) => {
      const d = e.payload;
      const msg = d?.message || "Unknown error";
      log(`⚠️ ${msg}`);

      // Promote critical errors to visible alerts
      if (msg.includes("Transcription unavailable") || msg.includes("Model")) {
        addAlert({
          level: "error",
          message: msg,
          actions: [{ label: "Speech settings", handler: () => navigate("settings:speech") }],
          dismissable: true,
        });
      } else if (msg.includes("closed") || msg.includes("Microphone disconnected")) {
        addAlert({
          level: "warning",
          message: msg,
          dismissable: true,
        });
      }
    });
    unlisteners.push(ue);

    // Handle LLM transcript corrections arriving after utterances are inserted.
    // Fetch only the corrected utterances by id range and patch them in-place.
    // We match against the last session id (which lives past `stop()`), since
    // corrections frequently complete after a session has stopped — especially
    // on the first run when the model is still loading.
    const uc = await listen("gravai:transcript-corrected", (e: any) => {
      const d = e.payload;
      if (!d?.utterance_ids?.length || !d.session_id) return;
      const visibleSid = get(currentSessionId) ?? get(lastSessionIdStore);
      if (d.session_id !== visibleSid) return;
      const minId: number = Math.min(...d.utterance_ids);
      invoke("get_transcript_since", { sessionId: d.session_id, afterId: minId - 1 })
        .then((utts: any) => {
          if (!Array.isArray(utts)) return;
          const byId = new Map(utts.map((u: any) => [u.id, u]));
          liveUtterances.update(current =>
            current.map(u => byId.has(u.id) ? { ...u, ...byId.get(u.id) } : u)
          );
        })
        .catch(() => {});
    });
    unlisteners.push(uc);

    const ub = await listen("gravai:bookmark", (e: any) => {
      bookmarkCount.update(n => n + 1);
      // Fires for both the bookmark button and the recording.bookmark
      // keyboard shortcut (handled window-side in StatusBar.svelte) —
      // either way, offer the one-line note prompt while it's still fresh.
      const d = e.payload;
      if (get(isRecording) && d?.session_id === get(currentSessionId) && d?.bookmark_id != null) {
        pendingBookmarkNote = { id: d.bookmark_id, offsetMs: d.offset_ms ?? 0 };
        bookmarkNoteValue = "";
        if (bookmarkNoteTimeout) clearTimeout(bookmarkNoteTimeout);
        bookmarkNoteTimeout = window.setTimeout(dismissBookmarkNote, 8000);
      }
    });
    unlisteners.push(ub);

    const usp = await listen("gravai:start-progress", (e: any) => {
      const msg = e.payload?.message as string;
      if (msg) {
        log(msg);
        // Update session title when calendar detects a meeting
        if (msg.startsWith("Meeting detected: ")) {
          sessionTitle = msg.replace("Meeting detected: ", "");
        }
        const replaced = msg.match(/Listening on (.+) instead\.$/)?.[1];
        if (replaced) {
          selectedMicName = replaced;
          invoke("update_config", {
            patch: { audio: { microphone: { enabled: true, device_name: replaced, device_index: -1 } } },
          }).catch(() => {});
          loadDevices();
        }
      }
    });
    unlisteners.push(usp);


    await loadDevices();
    // Load the live config for the source controls and the summary line
    try {
      const cfg: any = $config ?? await loadConfig();
      if (cfg.audio?.microphone?.device_name) {
        selectedMicName = cfg.audio.microphone.device_name;
      } else if ((cfg.audio?.microphone?.device_index ?? -1) >= 0) {
        const saved = micDevices.find(d => d.index === cfg.audio.microphone.device_index);
        selectedMicName = saved?.name ?? "";
      }
    } catch (_) {}
    checkMeetings();
    meetingPoll = window.setInterval(checkMeetings, 10000);
    devicePoll = window.setInterval(loadDevices, 3000);
    // If a session was started externally (e.g. by automation) while this page
    // wasn't mounted, resume the timer and transcript poll.
    if (get(isRecording) && !timerInterval) {
      const sid = get(currentSessionId);
      if (sid) {
        try {
          const sessions: any[] = await invoke("list_sessions");
          sessionTitle = sessions.find(s => s.id === sid)?.title ?? "";
        } catch (_) {}
      }
      lastUtteranceId = Math.max(0, ...get(liveUtterances).map(u => u.id));
      startTimer();
      startTranscriptPoll();
    } else if (!get(isRecording) && get(finishedSessionId) && !sessionTitle) {
      // Returning to an already-finished session after navigating away and
      // back (this is a fresh component instance — local state like the
      // title and sentiment didn't survive the unmount, only the stores did).
      const sid = get(finishedSessionId) as string;
      try {
        const sessions: any[] = await invoke("list_sessions");
        sessionTitle = sessions.find(s => s.id === sid)?.title ?? "";
      } catch (_) {}
      try { sentimentData = await invoke("get_session_sentiment", { sessionId: sid }); } catch (_) {}
      try { sessionBookmarks = await invoke("list_bookmarks", { sessionId: sid }); } catch (_) {}
    }
    try {
      const store: any = await invoke("get_shortcuts");
      bookmarkShortcutLabel = store?.bindings?.["recording.bookmark"]?.key_sequence ?? "";
    } catch (_) {}
    log("Gravai ready");
  });

  onDestroy(() => {
    stopTimer(); stopTranscriptPoll();
    if (meetingPoll) clearInterval(meetingPoll);
    if (devicePoll) clearInterval(devicePoll);
    for (const u of unlisteners) u();
  });

  function startTitleEdit() {
    titleEditValue = sessionTitle;
    editingTitle = true;
  }

  async function saveTitle() {
    const trimmed = titleEditValue.trim();
    editingTitle = false;
    if (trimmed === sessionTitle) return;
    const sid = $currentSessionId ?? $finishedSessionId;
    if (!sid) return;
    try {
      await invoke("rename_session", { sessionId: sid, title: trimmed });
      sessionTitle = trimmed;
    } catch (e) { log(`Title save error: ${e}`); }
  }

  // Split-screen resize
  let leftWidth = $state(440);

  function startResize(e: PointerEvent) {
    const startX = e.clientX;
    const startWidth = leftWidth;
    (e.target as HTMLElement).setPointerCapture(e.pointerId);

    function onMove(ev: PointerEvent) {
      leftWidth = Math.max(340, Math.min(680, startWidth + (ev.clientX - startX)));
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


{#snippet exportIndicator()}
  {#if exportAutoTranscript || exportRealtimeSave}
    <div class="export-indicator">
      {#if exportRealtimeSave}<span class="export-tag save"><Icon name="save" size={11}/> Saved while recording</span>{/if}
      {#if exportAutoTranscript}<span class="export-tag transcript"><Icon name="file-text" size={11}/> Transcript saved on stop</span>{/if}
    </div>
  {/if}
{/snippet}

{#snippet audioSources()}
  <div class="source-grid">
    <!-- Mic row -->
    <div class="source-row">
      <div class="source-row-top">
        <label class="source-toggle"><input type="checkbox" class="toggle" checked={micEnabled} onchange={(e) => setValue("audio.microphone.enabled", e.currentTarget.checked)} /> <Icon name="microphone" size={13}/> Microphone</label>
        <div class="source-meter"><div class="vu-meter"><div class="vu-fill" style="width: {vuMic}%"></div></div></div>
      </div>
      <div class="source-row-bottom">
        <div class="source-device">
          <select class="select select-sm" bind:value={selectedMicName} onchange={applyMicChoice}>
            <option value="">Default mic</option>
            {#if selectedMicName && !micDevices.some(d => d.name === selectedMicName)}
              <option value={selectedMicName}>{selectedMicName} (disconnected)</option>
            {/if}
            {#each micDevices as d}
              <option value={d.name}>{d.name}</option>
            {/each}
          </select>
        </div>
      </div>
    </div>
    <!-- System audio row -->
    <div class="source-row">
      <div class="source-row-top">
        <label class="source-toggle"><input type="checkbox" class="toggle" checked={sysEnabled} onchange={(e) => setValue("audio.system_audio.enabled", e.currentTarget.checked)} /> <Icon name="monitor" size={13}/> System Audio</label>
        <div class="source-meter"><div class="vu-meter"><div class="vu-fill" style="width: {vuSys}%"></div></div></div>
      </div>
      <div class="source-row-bottom">
        <div class="source-device">
          <AppPicker
            apps={runningApps}
            selected={selectedAppBundleId}
            onselect={(v) => setValue("audio.system_audio.app_bundle_id", v)}
          />
        </div>
      </div>
    </div>
  </div>
{/snippet}

{#snippet sentimentBlock()}
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
{/snippet}

{#snippet activityLog()}
  <div class="log-panel">
    {#each $activityLogs as line}<div>{line}</div>{/each}
  </div>
{/snippet}

<div class="recording-layout">
  <!-- Left panel: controls -->
  <div class="recording-left" style="flex: 0 0 {leftWidth}px">
    <div class="page-header">
      {#if $isRecording || finishedView}
        <div class="session-title-row">
          {#if editingTitle}
            <!-- svelte-ignore a11y_autofocus -->
            <input
              class="session-title-input"
              bind:value={titleEditValue}
              autofocus
              onblur={saveTitle}
              onkeydown={(e) => { if (e.key === "Enter") saveTitle(); if (e.key === "Escape") editingTitle = false; }}
            />
          {:else}
            <h2 class="session-title-display">{sessionTitle || "Recording"}</h2>
            <button class="title-edit-btn" onclick={startTitleEdit} title="Rename session" aria-label="Rename session">
              <Icon name="pencil" size={13}/>
            </button>
          {/if}
        </div>
      {:else}
        <h2>Recording</h2>
      {/if}
      <span class="timer">{timer}</span>
    </div>
    {#if finishedView && $liveSummary?.tldr}
      <p class="finished-tldr">{$liveSummary.tldr}</p>
    {/if}

    <RunBar />

    <!-- Transport -->
    <div class="transport">
      <button class="transport-btn record" class:active={$isRecording && !$isPaused} disabled={$isRecording || starting} onclick={start} title="Record"><Icon name={starting ? "spinner" : "record"} size={22}/></button>
      <button class="transport-btn pause" disabled={!$isRecording} onclick={togglePause} title={$isPaused ? "Resume" : "Pause"}><Icon name={$isPaused ? "play" : "pause"} size={20}/></button>
      <button class="transport-btn stop" disabled={!$isRecording && !starting} onclick={stop} title="Stop"><Icon name="stop" size={20}/></button>
      <button class="transport-btn bookmark" disabled={!$isRecording} onclick={addBookmark} title={bookmarkShortcutLabel ? `Add bookmark (${prettyShortcut(bookmarkShortcutLabel)})` : "Add bookmark"}>
        <Icon name="bookmark" size={18}/>{#if $bookmarkCount > 0}<span class="bookmark-badge">{$bookmarkCount}</span>{/if}
      </button>
      <span class="status-badge" class:recording={$isRecording && !$isPaused} class:paused={$isRecording && $isPaused} class:idle={!$isRecording && !starting} class:starting={starting}>
        {starting ? "Starting..." : $isRecording ? ($isPaused ? "Paused" : "Recording") : "Idle"}
      </span>
    </div>

    {#if pendingBookmarkNote}
      {#key pendingBookmarkNote.id}
        <!-- svelte-ignore a11y_autofocus -->
        <input
          class="bookmark-note-prompt"
          placeholder="Add a note… Enter to save, Esc to skip"
          autofocus
          bind:value={bookmarkNoteValue}
          onkeydown={(e) => { if (e.key === "Enter") saveBookmarkNote(); if (e.key === "Escape") dismissBookmarkNote(); }}
          onblur={dismissBookmarkNote}
        />
      {/key}
    {/if}

    <!-- "Is sound coming in?" — one compact VU row per source, always visible while recording -->
    {#if $isRecording}
      <div class="vu-row">
        <div class="vu-row-item">
          <Icon name="microphone" size={13}/>
          <div class="vu-meter"><div class="vu-fill" style="width: {vuMic}%"></div></div>
          {#if noSoundMic}<span class="no-sound-hint" title={$sourceWarnings.microphone?.message ?? ""}>No sound from Mic</span>{/if}
        </div>
        <div class="vu-row-item">
          <Icon name="monitor" size={13}/>
          <div class="vu-meter"><div class="vu-fill" style="width: {vuSys}%"></div></div>
          {#if noSoundSys}<span class="no-sound-hint" title={$sourceWarnings.system_audio?.message ?? ""}>No sound from System</span>{/if}
        </div>
      </div>
    {/if}

    <!-- Model load and errors. In the finished view this also covers "Summarizing…". -->
    <LlmStatusBanner scope={finishedView ? "all" : "status"} />

    {#if finishedView}
      <!-- Finished session: outcome, then the session's permanent home -->
      <div class="finished-actions">
        <button class="btn btn-sm btn-accent" onclick={openInArchive}><Icon name="archive" size={13}/> Open in Archive</button>
        <button class="btn btn-sm btn-ghost" onclick={copyMarkdownPath} title="Copy the path of the transcript's Markdown file">
          <Icon name={copiedPath ? "check" : "file"} size={13}/> {copiedPath ? "Copied path" : "Copy path"}
        </button>
        <button class="btn btn-sm btn-ghost" onclick={newRecording}><Icon name="plus" size={13}/> New recording</button>
      </div>
      {#if copyPathError}<p class="copy-path-error" role="alert">Copy path failed: {copyPathError}</p>{/if}

      <div class="card outcome-card">
        {#if finalizing}
          <div class="empty-state">Checking transcript…</div>
        {:else if emptyTranscriptAfterStop}
          <div class="empty-state">No speech was captured.</div>
        {:else if summaryLoading}
          <div class="empty-state">Summarizing…</div>
        {:else if $liveSummary?.key_decisions?.length || $liveSummary?.action_items?.length}
          <div class="summary-content">
            {#if $liveSummary.key_decisions?.length}
              <h4>Key Decisions</h4>
              <ul>{#each $liveSummary.key_decisions.slice(0, 5) as d}<li>{d}</li>{/each}</ul>
              {#if $liveSummary.key_decisions.length > 5}
                <button class="link-more" onclick={openInArchive}>{$liveSummary.key_decisions.length - 5} more in Archive</button>
              {/if}
            {/if}
            {#if $liveSummary.action_items?.length}
              <h4>Action Items</h4>
              <ul>{#each $liveSummary.action_items.slice(0, 5) as a}<li>{a.description} {#if a.owner}<span class="action-owner">@{a.owner}</span>{/if}</li>{/each}</ul>
              {#if $liveSummary.action_items.length > 5}
                <button class="link-more" onclick={openInArchive}>{$liveSummary.action_items.length - 5} more in Archive</button>
              {/if}
            {/if}
          </div>
        {:else}
          <div class="summary-error">
            <div class="summary-error-line">
              {#if summaryError}<Icon name="alert-triangle" size={13}/>{/if}
              <span class="summary-error-msg">Summary unavailable{#if summaryError}: {summaryError}{/if}</span>
              <button class="btn btn-xs btn-ghost" onclick={() => generateSummary()} disabled={summaryLoading}>
                <Icon name="refresh" size={12}/> Retry
              </button>
            </div>
          </div>
        {/if}
      </div>

      <!-- Sentiment, moved below the outcome block -->
      {@render sentimentBlock()}

      {#if sessionBookmarks.length > 0}
        <details class="card collapsible" open>
          <summary class="card-header">Bookmarks ({sessionBookmarks.length})</summary>
          <div class="finished-bookmarks-list">
            {#each sessionBookmarks as b}
              <div class="finished-bookmark-row">
                <span class="finished-bookmark-time">{fmtOffsetMs(b.offset_ms)}</span>
                <span class="finished-bookmark-note">{b.note || "—"}</span>
              </div>
            {/each}
          </div>
        </details>
      {/if}
    {:else if $isRecording}
      <!-- Everything else collapses into one closed-by-default card while recording,
           so the three questions above stay in view. -->
      <details class="card collapsible">
        <summary class="card-header">Setup &amp; diagnostics</summary>
        {@render exportIndicator()}
        {@render audioSources()}
        {@render sentimentBlock()}
        <div class="card-header section">Activity Log</div>
        {@render activityLog()}
      </details>
    {:else}
      <!-- Idle: today's layout, ungrouped -->
      {@render exportIndicator()}
      <details class="card collapsible" open>
        <summary class="card-header">Audio Sources</summary>
        {@render audioSources()}
      </details>
      <details class="card collapsible" open>
        <summary class="card-header">Activity Log</summary>
        {@render activityLog()}
      </details>
    {/if}
  </div>

  <!-- Drag resizer -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="recording-resizer" onpointerdown={startResize}></div>

  <!-- Right panel: live transcript -->
  <div class="recording-right">
    <div class="transcript-panel-header">
      <span class="transcript-panel-title">Live Transcript</span>
      <button type="button" class="header-toggle-btn" onclick={() => autoScrollEnabled.set(!$autoScrollEnabled)}>
        <Icon name={$autoScrollEnabled ? "checkbox-checked" : "checkbox-empty"} size={13}/> Auto-scroll
      </button>
    </div>
    <div class="transcript-panel-body">
      <TranscriptView utterances={$liveUtterances} autoScroll={$autoScrollEnabled} {showEmotions} sessionId={$currentSessionId ?? $lastSessionIdStore} sessionStartedAt={$sessionStartTime ? new Date($sessionStartTime).toISOString() : ""} />
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
    overflow-x: hidden;
    padding: 16px 8px 16px 20px;
    min-width: 340px;
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
  .summary-error {
    padding: 8px 14px 10px;
    background: color-mix(in srgb, var(--danger) 6%, transparent);
    border-bottom: 1px solid color-mix(in srgb, var(--danger) 25%, var(--border-subtle));
  }
  .summary-error-line {
    display: flex; align-items: center; gap: 6px;
    color: var(--danger);
    font-size: 12px; font-weight: 500;
  }
  .summary-error-msg { flex: 1; }
  .session-title-row {
    display: flex; align-items: center; gap: 6px; flex: 1; min-width: 0;
  }
  .session-title-display {
    font-size: 22px; font-weight: 700;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .session-title-input {
    font-size: 18px; font-weight: 600; flex: 1; min-width: 0;
    background: var(--bg-elevated); border: 1px solid var(--accent-dim);
    border-radius: var(--radius-sm); color: var(--text-primary);
    padding: 2px 8px; font-family: inherit;
  }
  .title-edit-btn {
    background: none; border: none; cursor: pointer; color: var(--text-tertiary);
    padding: 2px 4px; border-radius: 4px; flex-shrink: 0;
    display: flex; align-items: center;
  }
  .title-edit-btn:hover { color: var(--text-secondary); background: var(--bg-secondary); }
  .vu-row {
    display: flex; gap: 14px; padding: 2px 2px 0;
  }
  .vu-row-item {
    display: flex; align-items: center; gap: 6px; flex: 1; min-width: 0;
    color: var(--text-tertiary);
  }
  .vu-row-item .vu-meter { flex: 1; }
  .no-sound-hint {
    font-size: 10.5px; color: var(--warning); white-space: nowrap; flex-shrink: 0;
  }
  .finished-tldr {
    font-size: 13px; color: var(--text-secondary); margin: 2px 0 0; line-height: 1.4;
  }
  .bookmark-note-prompt {
    width: 100%; font-size: 12px; background: var(--bg-elevated);
    border: 1px solid var(--accent-dim); border-radius: var(--radius-sm);
    color: var(--text-primary); padding: 5px 10px; font-family: inherit;
    margin: -2px 0 2px;
  }
  .finished-bookmarks-list { padding: 4px 0; display: flex; flex-direction: column; }
  .finished-bookmark-row {
    display: flex; align-items: center; gap: 8px; font-size: 12px; padding: 5px 14px;
  }
  .finished-bookmark-time {
    font-family: "SF Mono", monospace; font-size: 11px; font-weight: 600;
    color: var(--accent); white-space: nowrap; min-width: 36px;
  }
  .finished-bookmark-note { color: var(--text-secondary); flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .finished-actions {
    display: flex; gap: 8px;
  }
  .copy-path-error { font-size: 11px; color: var(--danger); margin: 0; }
  .outcome-card { padding: 0; }
  .outcome-card .empty-state { padding: 16px; }
  .link-more {
    background: none; border: none; cursor: pointer; color: var(--accent);
    font-size: 11px; font-weight: 500; padding: 0 16px 10px; text-align: left;
  }
  .link-more:hover { text-decoration: underline; }
  .transport-btn.bookmark { position: relative; }
  .bookmark-badge {
    position: absolute; top: -4px; right: -4px;
    background: var(--accent); color: var(--bg-primary);
    font-size: 9px; font-weight: 700; line-height: 1;
    min-width: 16px; height: 16px;
    display: flex; align-items: center; justify-content: center;
    border-radius: 8px; padding: 0 4px;
  }
</style>
