<script lang="ts">
  import type { Snippet } from "svelte";
  // A titled card of rows inside a settings section.
  let { title = "", note = "", children }: { title?: string; note?: string; children: Snippet } = $props();
</script>

<section class="grp">
  {#if title}<h3>{title}</h3>{/if}
  {#if note}<p class="grp-note">{note}</p>{/if}
  <div class="grp-card">{@render children()}</div>
</section>

<style>
  .grp { display: flex; flex-direction: column; gap: 8px; }
  h3 { margin: 0; font-size: 12px; font-weight: 600; color: var(--text-secondary); letter-spacing: 0.3px; }
  .grp-note { margin: 0; font-size: 12px; color: var(--text-tertiary); line-height: 1.45; }
  .grp-card { background: var(--bg-secondary); border: 1px solid var(--border-subtle); border-radius: var(--radius-md); }
</style>
