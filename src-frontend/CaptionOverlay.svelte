<script lang="ts">
  import { onMount } from "svelte";
  import { invoke, listen } from "./lib/tauri";
  let data = $state<any>(null);
  onMount(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    (async () => {
      const off = await listen("gravai:captions", e => { data = e.payload; });
      if (disposed) { off(); return; }
      unlisten = off;
      data = await invoke("get_live_captions");
    })();
    return () => { disposed = true; unlisten?.(); };
  });
  let config = $derived(data?.config);
  let lines = $derived(data?.buffer?.lines?.slice(-(config?.max_lines ?? 3)) ?? []);
</script>

<div class="captions" style:background={`rgba(12, 14, 18, ${config?.opacity ?? 0.9})`} style:font-size={`${config?.font_pt ?? 22}pt`}>
  <div class="status">Gravai · {data?.buffer?.state || "inactive"}</div>
  <div role="log" aria-live="polite" class="lines" style={`-webkit-line-clamp: ${config?.max_lines ?? 3}`}>
    {#if lines.length}{#each lines as line}<div class="line">{line}</div>{/each}{:else}{data?.buffer?.state === "recording" ? "Listening…" : "No live captions"}{/if}
  </div>
</div>
<style>
  :global(html.live-captions), :global(body.live-captions), :global(body.live-captions #app) { margin: 0; width: 100%; height: 100%; background: transparent; }
  .captions { box-sizing: border-box; height: 100%; padding: 12px 18px; color: white; font-family: -apple-system, sans-serif; }
  .status { font-size: 11px; margin-bottom: 8px; color: #ced4df; }
  .line { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .lines { white-space: pre-line; line-height: 1.5; display: -webkit-box; -webkit-box-orient: vertical; overflow: hidden; }
</style>
