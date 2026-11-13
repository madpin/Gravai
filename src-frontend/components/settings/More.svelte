<script lang="ts">
  import type { Snippet } from "svelte";
  let { label = "More options", children }: { label?: string; children: Snippet } = $props();
</script>

<details class="more">
  <summary>{label}</summary>
  <div class="more-body">{@render children()}</div>
</details>

<style>
  .more { border-top: 1px solid var(--border-subtle); }
  .more > summary {
    list-style: none; cursor: pointer; padding: 10px 18px;
    font-size: 12px; color: var(--text-secondary); display: flex; align-items: center; gap: 6px;
  }
  .more > summary::-webkit-details-marker { display: none; }
  .more > summary::before { content: "›"; display: inline-block; transition: transform 0.15s; font-size: 14px; }
  .more[open] > summary::before { transform: rotate(90deg); }
  .more > summary:hover { color: var(--text-primary); }
  .more-body { border-top: 1px solid var(--border-subtle); background: rgba(0,0,0,0.08); }
</style>
