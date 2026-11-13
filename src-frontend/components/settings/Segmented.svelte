<script lang="ts">
  let {
    options,
    value,
    onchange,
  }: { options: Array<[string, string]>; value: string; onchange?: (value: string) => void } = $props();
</script>

<div class="seg" role="radiogroup">
  {#each options as [v, label]}
    <button type="button" role="radio" aria-checked={value === v} class:active={value === v} onclick={() => value !== v && onchange?.(v)}>{label}</button>
  {/each}
</div>

<style>
  .seg { display: inline-flex; background: var(--bg-base); border: 1px solid var(--border); border-radius: 8px; padding: 2px; gap: 2px; }
  .seg button {
    border: none; background: none; color: var(--text-secondary); font: inherit; font-size: 12px;
    padding: 4px 12px; border-radius: 6px; cursor: pointer;
  }
  .seg button:hover { color: var(--text-primary); }
  .seg button.active { background: var(--bg-elevated); color: var(--text-primary); font-weight: 600; }
</style>
