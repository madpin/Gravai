<script lang="ts">
  let {
    value,
    min,
    max,
    step = 1,
    unit = "",
    onsave,
  }: { value: number; min?: number; max?: number; step?: number; unit?: string; onsave: (value: number) => void } = $props();
  let draft = $state(0);
  $effect(() => { draft = value; });
  function commit() {
    let v = Number(draft);
    if (!Number.isFinite(v)) { draft = value; return; }
    if (min != null) v = Math.max(min, v);
    if (max != null) v = Math.min(max, v);
    draft = v;
    if (v !== value) onsave(v);
  }
</script>

<span class="num">
  <input class="input input-narrow" type="number" {min} {max} {step} bind:value={draft} onblur={commit}
    onkeydown={(e) => { if (e.key === "Enter") (e.currentTarget as HTMLInputElement).blur(); }} />
  {#if unit}<span class="unit">{unit}</span>{/if}
</span>

<style>
  .num { display: inline-flex; align-items: center; gap: 6px; }
  .num input { width: 80px; }
  .unit { font-size: 12px; color: var(--text-tertiary); }
</style>
