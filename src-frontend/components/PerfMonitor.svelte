<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "../lib/tauri";
  import Icon from "./Icon.svelte";

  let snap = $state<any>(null);
  let interval: number | null = null;

  function fmtUptime(secs: number): string {
    const m = Math.floor(secs / 60);
    if (m < 60) return `${m}m`;
    const h = Math.floor(m / 60);
    const rm = m % 60;
    return `${h}h ${rm}m`;
  }

  async function refresh() {
    try { snap = await invoke("get_perf_snapshot"); } catch (_) {}
  }

  onMount(() => {
    refresh();
    interval = window.setInterval(refresh, 5000);
  });

  onDestroy(() => {
    if (interval) clearInterval(interval);
  });

  let warn = $derived(snap && (snap.cpu_pct > 60 || snap.memory_pct > 80));
</script>

{#if snap}
  <div class="perf-bar" class:warn>
    <span title="CPU usage"><Icon name="settings" size={10}/> {snap.cpu_pct.toFixed(1)}%</span>
    <span class="sep">|</span>
    <span title="Memory: {snap.rss_mb.toFixed(0)}MB of {(snap.total_memory_gb * 1024).toFixed(0)}MB"><Icon name="cpu" size={10}/> {snap.rss_mb.toFixed(0)}MB</span>
    <span class="sep">|</span>
    <span title="Uptime"><Icon name="clock" size={10}/> {fmtUptime(snap.uptime_seconds)}</span>
  </div>
{/if}

<style>
  .perf-bar {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 10px;
    color: var(--text-tertiary);
    white-space: nowrap;
  }
  .perf-bar.warn { color: var(--warning, #f59e0b); }
  .sep { opacity: 0.4; }
</style>
