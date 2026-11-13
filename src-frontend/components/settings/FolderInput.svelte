<script lang="ts">
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  let { value, placeholder = "Default", onsave }: { value: string | null; placeholder?: string; onsave: (value: string | null) => void } = $props();
  async function browse() {
    const picked = await openDialog({ directory: true });
    if (picked) onsave(picked as string);
  }
</script>

<span class="folder">
  <span class="path" title={value ?? ""}>{value || placeholder}</span>
  <button class="btn btn-xs btn-ghost" onclick={browse}>Choose…</button>
  {#if value}<button class="btn btn-xs btn-ghost" onclick={() => onsave(null)}>Reset</button>{/if}
</span>

<style>
  .folder { display: inline-flex; align-items: center; gap: 6px; max-width: 360px; }
  .path { font-size: 12px; color: var(--text-secondary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 200px; direction: rtl; text-align: left; }
</style>
