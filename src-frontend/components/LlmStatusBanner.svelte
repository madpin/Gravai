<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { llmStatus } from "../lib/store";
  import Icon from "./Icon.svelte";

  // Tick once a second so the elapsed counter and the *interpolated* progress
  // bar both update smoothly between server-side ticks (which arrive ~1 Hz).
  let now = $state(Date.now());
  let interval: number | null = null;

  onMount(() => {
    interval = window.setInterval(() => { now = Date.now(); }, 250);
  });
  onDestroy(() => { if (interval) clearInterval(interval); });

  function fmt(secs: number): string {
    if (secs < 60) return `${secs}s`;
    const m = Math.floor(secs / 60);
    const s = secs % 60;
    return `${m}m ${s.toString().padStart(2, "0")}s`;
  }

  let elapsedS = $derived.by(() => {
    const started = $llmStatus.started_at;
    return started ? Math.floor((now - started) / 1000) : 0;
  });

  // Visible whenever a load is in progress or has just failed.
  let visible = $derived(
    $llmStatus.state === "loading"
    || $llmStatus.state === "first_run"
    || $llmStatus.state === "progress"
    || $llmStatus.state === "error",
  );

  let isFirstRun = $derived($llmStatus.state === "first_run");
  let isError = $derived($llmStatus.state === "error");
  let model = $derived($llmStatus.model_id ?? "local model");

  // Progress bar value: prefer the server-reported progress; otherwise fall
  // back to a smooth time-based interpolation against the eta hint. Capped at
  // 0.95 until `ready` event lands so we never oversell completion.
  let pct = $derived.by(() => {
    if (isError) return 0;
    const reported = $llmStatus.progress;
    const eta = $llmStatus.eta_seconds ?? 0;
    if (typeof reported === "number") {
      // Interpolate forward by up to ~1 s worth of expected progress so the
      // bar doesn't look frozen between backend ticks.
      const bonus = eta > 0 ? Math.min(0.02, 1 / eta) : 0;
      return Math.min(0.95, reported + bonus);
    }
    if (eta > 0 && elapsedS > 0) {
      return Math.min(0.95, elapsedS / eta);
    }
    return 0;
  });

  let pctLabel = $derived(`${Math.round(pct * 100)}%`);

  let etaRemaining = $derived.by(() => {
    const eta = $llmStatus.eta_seconds ?? 0;
    if (!eta) return null;
    const remaining = Math.max(0, eta - elapsedS);
    return remaining;
  });

  let phase = $derived($llmStatus.phase ?? null);
</script>

{#if visible}
  <div class="llm-banner" class:first-run={isFirstRun} class:error={isError}>
    <span class="llm-icon">
      <Icon name={isError ? "alert-triangle" : "spinner"} size={14}/>
    </span>
    <div class="llm-text">
      <div class="llm-title">
        {#if isError}
          Local LLM error
        {:else if isFirstRun}
          Preparing local model: <code>{model}</code>
        {:else}
          Loading local model: <code>{model}</code>
        {/if}
      </div>

      {#if !isError}
        <div class="llm-progress-wrap">
          <div class="llm-progress" class:first-run={isFirstRun}>
            <div class="llm-progress-fill" style="width: {pct * 100}%"></div>
          </div>
          <span class="llm-pct">{pctLabel}</span>
        </div>

        <div class="llm-message">
          {#if phase}<span class="llm-phase">{phase}</span> · {/if}
          <span class="llm-elapsed">elapsed {fmt(elapsedS)}</span>
          {#if etaRemaining !== null && etaRemaining > 0}
            · <span class="llm-eta">~{fmt(etaRemaining)} left</span>
          {/if}
        </div>

        {#if isFirstRun && $llmStatus.message}
          <div class="llm-message llm-fineprint">{$llmStatus.message}</div>
        {/if}
      {:else if $llmStatus.message}
        <div class="llm-message">{$llmStatus.message}</div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .llm-banner {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 10px 12px;
    background: var(--bg-elevated);
    border: 1px solid var(--border-subtle);
    border-left: 3px solid var(--accent);
    border-radius: var(--radius-sm);
    font-size: 12px;
    color: var(--text-secondary);
  }
  .llm-banner.first-run {
    border-left-color: var(--warning, #f59e0b);
    background: color-mix(in srgb, var(--warning, #f59e0b) 7%, var(--bg-elevated));
  }
  .llm-banner.error {
    border-left-color: var(--danger);
    background: color-mix(in srgb, var(--danger) 7%, var(--bg-elevated));
    color: var(--text-primary);
  }
  .llm-icon {
    flex-shrink: 0;
    color: var(--accent);
    margin-top: 1px;
    display: flex;
    align-items: center;
  }
  .llm-banner.first-run .llm-icon { color: var(--warning, #f59e0b); }
  .llm-banner.error .llm-icon { color: var(--danger); }

  .llm-text {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    flex: 1;
  }
  .llm-title {
    font-weight: 600;
    color: var(--text-primary);
  }
  .llm-title code {
    background: var(--bg-secondary);
    padding: 1px 5px;
    border-radius: 3px;
    font-size: 11px;
    font-family: var(--font-mono, ui-monospace, monospace);
  }

  /* Progress bar */
  .llm-progress-wrap {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 2px;
  }
  .llm-progress {
    flex: 1;
    height: 6px;
    background: var(--bg-secondary);
    border-radius: 999px;
    overflow: hidden;
    position: relative;
  }
  .llm-progress-fill {
    height: 100%;
    background: var(--accent);
    border-radius: 999px;
    transition: width 0.5s ease-out;
  }
  .llm-progress.first-run .llm-progress-fill {
    background: var(--warning, #f59e0b);
  }
  .llm-pct {
    font-variant-numeric: tabular-nums;
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
    min-width: 36px;
    text-align: right;
  }

  .llm-message {
    color: var(--text-tertiary);
    font-size: 11px;
    line-height: 1.4;
  }
  .llm-phase { color: var(--text-secondary); font-weight: 500; }
  .llm-elapsed,
  .llm-eta { font-variant-numeric: tabular-nums; }
  .llm-fineprint { font-size: 10.5px; opacity: 0.85; }
</style>
