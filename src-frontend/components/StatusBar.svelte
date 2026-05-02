<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke, listen } from "../lib/tauri";
  import Icon from "./Icon.svelte";
  import {
    isRecording, isPaused, activityLogs, healthStatus, currentPage,
    currentSessionId, sessionStartTime, lastSessionId as lastSessionIdStore,
    liveUtterances, liveSummary, llmStatus,
  } from "../lib/store";

  let snap = $state<any>(null);
  let interval: number | null = null;
  const unlisteners: (() => void)[] = [];

  async function refresh() {
    try { snap = await invoke("get_perf_snapshot"); } catch (_) {}
  }

  async function autoStart() {
    if ($isRecording) return;
    try {
      const result: any = await invoke("start_session");
      isRecording.set(true);
      isPaused.set(false);
      currentSessionId.set(result.id);
      sessionStartTime.set(Date.now());
      lastSessionIdStore.set(result.id);
      liveUtterances.set([]);
      liveSummary.set(null);
      currentPage.set("recording");
    } catch (e) {
      activityLogs.update(l => [...l.slice(-99), `[Auto] Start failed: ${e}`]);
    }
  }

  onMount(async () => {
    refresh();
    interval = window.setInterval(refresh, 5000);
    unlisteners.push(await listen("gravai:stop-session", () => stop()));
    unlisteners.push(await listen("gravai:automation-start", () => autoStart()));
  });
  onDestroy(() => {
    if (interval) clearInterval(interval);
    for (const u of unlisteners) u();
  });

  let lastLog = $derived($activityLogs[$activityLogs.length - 1] || "");
  let warn = $derived(snap && (snap.cpu_pct > 60 || snap.memory_pct > 80));

  // Local LLM status indicator — show only while loading or on error.
  let llmBusy = $derived(
    $llmStatus.state === "loading"
    || $llmStatus.state === "first_run"
    || $llmStatus.state === "progress"
    || $llmStatus.state === "error",
  );
  let llmPct = $derived(
    typeof $llmStatus.progress === "number"
      ? `${Math.round($llmStatus.progress * 100)}%`
      : "",
  );

  function fmtUptime(secs: number): string {
    const m = Math.floor(secs / 60);
    if (m < 60) return `${m}m`;
    return `${Math.floor(m / 60)}h ${m % 60}m`;
  }

  async function togglePause() {
    try {
      if ($isPaused) { await invoke("resume_session"); isPaused.set(false); }
      else { await invoke("pause_session"); isPaused.set(true); }
    } catch (_) {}
  }

  async function stop() {
    try {
      const result: any = await invoke("stop_session");
      isRecording.set(false);
      isPaused.set(false);
      currentSessionId.set(null);
      sessionStartTime.set(null);
    } catch (_) {}
  }
</script>

<footer class="status-bar">
  <!-- Transport + recording state -->
  <div class="sb-section sb-left">
    {#if $isRecording}
      <span class="rec-pulse"></span>
      <span class="sb-state">{$isPaused ? "Paused" : "Recording"}</span>
      <button class="sb-btn" onclick={togglePause} title={$isPaused ? "Resume" : "Pause"}>
        <Icon name={$isPaused ? "play" : "pause"} size={11}/>
      </button>
      <button class="sb-btn" onclick={stop} title="Stop recording"><Icon name="stop" size={11}/></button>
    {:else}
      <span class="sb-idle-dot"></span>
      <button
        class="sb-btn sb-btn-muted"
        onclick={() => currentPage.set("recording")}
        title="Go to Recording tab"
      ><Icon name="record" size={11}/></button>
      <span class="sb-state muted">Idle</span>
    {/if}
  </div>

  <!-- Last activity log + LLM status -->
  <div class="sb-section sb-center">
    {#if llmBusy}
      <span
        class="sb-llm"
        class:first-run={$llmStatus.state === "first_run"}
        class:err={$llmStatus.state === "error"}
        title={$llmStatus.phase ?? $llmStatus.message ?? "Loading local LLM…"}
      >
        <Icon name={$llmStatus.state === "error" ? "alert-triangle" : "spinner"} size={11}/>
        {#if $llmStatus.state === "first_run"}
          Preparing {$llmStatus.model_id ?? "model"}
        {:else if $llmStatus.state === "error"}
          LLM error
        {:else}
          Loading {$llmStatus.model_id ?? "model"}
        {/if}
        {#if llmPct}<span class="sb-llm-pct">· {llmPct}</span>{/if}
      </span>
    {:else if lastLog}
      <span class="sb-log" title={lastLog}>{lastLog}</span>
    {/if}
  </div>

  <!-- Resources + health -->
  <div class="sb-section sb-right">
    {#if snap}
      <span class="sb-metric" class:warn title="Gravai CPU usage">{snap.cpu_pct.toFixed(1)}% CPU</span>
      <span class="sb-sep">·</span>
      <span class="sb-metric" class:warn title="Gravai memory">{snap.rss_mb.toFixed(0)} MB</span>
      <span class="sb-sep">·</span>
      <span class="sb-metric" title="Uptime">{fmtUptime(snap.uptime_seconds)}</span>
      <span class="sb-sep">·</span>
    {/if}
    <div class="sb-health-dot" class:green={$healthStatus === "ok"} class:yellow={$healthStatus === "warn"} class:red={$healthStatus === "error"} title="System health: {$healthStatus}"></div>
    <span class="sb-version">v..1</span>
  </div>
</footer>

<style>
  .status-bar {
    height: 28px;
    background: var(--bg-primary);
    border-top: 1px solid var(--border-subtle);
    display: flex;
    align-items: center;
    padding: 0 12px;
    gap: 0;
    flex-shrink: 0;
    user-select: none;
    -webkit-user-select: none;
  }
  .sb-section {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    color: var(--text-tertiary);
    white-space: nowrap;
    overflow: hidden;
  }
  .sb-left  { flex: 0 0 auto; min-width: 130px; }
  .sb-center { flex: 1; min-width: 0; padding: 0 16px; }
  .sb-right  { flex: 0 0 auto; margin-left: auto; }

  /* Recording state */
  .rec-pulse {
    width: 7px; height: 7px; border-radius: 50%; flex-shrink: 0;
    background: var(--danger);
    animation: pulse 1.2s ease-in-out infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50%       { opacity: 0.3; }
  }
  .sb-idle-dot {
    width: 7px; height: 7px; border-radius: 50%; flex-shrink: 0;
    background: var(--text-tertiary); opacity: 0.4;
  }
  .sb-state { font-size: 11px; font-weight: 600; color: var(--text-secondary); }
  .sb-state.muted { color: var(--text-tertiary); font-weight: 400; }

  /* Buttons */
  .sb-btn {
    background: none; border: none; padding: 1px 5px; cursor: pointer;
    font-size: 11px; color: var(--text-secondary); border-radius: 3px;
    line-height: 1; transition: background 0.1s, color 0.1s;
  }
  .sb-btn:hover { background: var(--bg-elevated); color: var(--text-primary); }
  .sb-btn-muted { color: var(--text-tertiary); }

  /* Last log */
  .sb-log {
    font-size: 11px; color: var(--text-tertiary);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    max-width: 100%;
  }

  /* LLM status pill (replaces .sb-log while busy) */
  .sb-llm {
    display: inline-flex; align-items: center; gap: 5px;
    font-size: 11px; font-weight: 500;
    padding: 1px 8px; border-radius: 10px;
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    color: var(--accent);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .sb-llm.first-run {
    background: color-mix(in srgb, var(--warning, #f59e0b) 14%, transparent);
    color: var(--warning, #f59e0b);
  }
  .sb-llm.err {
    background: color-mix(in srgb, var(--danger) 14%, transparent);
    color: var(--danger);
  }
  .sb-llm-pct {
    font-variant-numeric: tabular-nums;
    opacity: 0.85;
  }

  /* Resources */
  .sb-metric { font-size: 11px; }
  .sb-metric.warn { color: var(--warning, #f59e0b); }
  .sb-sep { opacity: 0.35; }

  /* Health */
  .sb-health-dot {
    width: 7px; height: 7px; border-radius: 50%; flex-shrink: 0;
    transition: background 0.3s;
  }
  .sb-health-dot.green { background: var(--success); box-shadow: 0 0 5px var(--success); }
  .sb-health-dot.yellow { background: var(--warning); box-shadow: 0 0 5px var(--warning); }
  .sb-health-dot.red { background: var(--danger); box-shadow: 0 0 5px var(--danger); }

  .sb-version { font-size: 10px; color: var(--text-tertiary); opacity: 0.6; }
</style>
