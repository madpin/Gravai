<script lang="ts">
  import type { Snippet } from "svelte";
  import { isRecording } from "../../lib/store";

  // One setting: what it is, what it costs, and the control.
  let {
    label,
    hint = "",
    cost = "",
    next = false,
    stacked = false,
    children,
  }: {
    label: string;
    hint?: string;
    cost?: string;
    /** Takes effect at the next recording. Tagged while one is running. */
    next?: boolean;
    /** Put the control under the label (text areas, lists). */
    stacked?: boolean;
    children?: Snippet;
  } = $props();
</script>

<div class="s-row" class:stacked>
  <div class="s-info">
    <span class="s-label">
      {label}
      {#if next && $isRecording}<span class="s-next">Next recording</span>{/if}
    </span>
    {#if hint}<span class="s-hint">{hint}</span>{/if}
    {#if cost}<span class="s-cost">{cost}</span>{/if}
  </div>
  {#if children}<div class="s-control">{@render children()}</div>{/if}
</div>

<style>
  .s-row {
    display: flex; align-items: center; gap: 16px;
    padding: 12px 18px; border-bottom: 1px solid var(--border-subtle);
  }
  .s-row:last-child { border-bottom: none; }
  .s-row.stacked { flex-direction: column; align-items: stretch; gap: 8px; }
  .s-info { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 3px; }
  .s-label { font-size: 13px; font-weight: 500; color: var(--text-primary); display: flex; align-items: center; gap: 8px; }
  .s-hint { font-size: 12px; color: var(--text-secondary); line-height: 1.45; }
  .s-cost { font-size: 11px; color: var(--text-tertiary); line-height: 1.4; }
  .s-next {
    font-size: 10px; font-weight: 600; padding: 1px 6px; border-radius: 4px;
    background: rgba(251,191,36,0.12); color: var(--warning);
  }
  .s-control { flex-shrink: 0; display: flex; align-items: center; gap: 8px; }
  .stacked .s-control { display: block; }
  .stacked .s-control > :global(.picker) { margin: 0 -18px 0 -12px; }
</style>
