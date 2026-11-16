<script lang="ts">
  import { config, setValue } from "../../lib/config";
  import { invoke } from "../../lib/tauri";
  import Group from "./Group.svelte";
  import Row from "./Row.svelte";
  import Toggle from "./Toggle.svelte";
  import NumberInput from "./NumberInput.svelte";
  let c = $derived($config?.live_captions);
</script>
{#if c}
  <Group title="Live captions" note="Finalized speech on this Mac. Off by default. The overlay uses the current display and Space; menu bar text is visible to anyone looking at your screen.">
    <Row label="Enable live captions"><Toggle label="Enable live captions" checked={c.enabled} onchange={v => setValue("live_captions.enabled", v)} /></Row>
    {#if c.enabled}
      <Row label="Show in"><select aria-label="Caption mode" value={c.mode} onchange={e => setValue("live_captions.mode", e.currentTarget.value)}><option value="overlay">Overlay</option><option value="menu_bar">Menu bar</option><option value="both">Both</option></select></Row>
      <Row label="Position"><select aria-label="Overlay position" value={c.position} onchange={e => setValue("live_captions.position", e.currentTarget.value)}>{#each ["top_left", "top_right", "bottom_left", "bottom_right"] as p}<option value={p}>{p.replaceAll("_", " ")}</option>{/each}</select></Row>
      <Row label="Opacity"><NumberInput value={c.opacity} min={0.2} max={1} step={0.05} onsave={v => setValue("live_captions.opacity", v)} /></Row>
      <Row label="Font size (pt)"><NumberInput value={c.font_pt} min={12} max={64} onsave={v => setValue("live_captions.font_pt", v)} /></Row>
      <Row label="Visible lines"><NumberInput value={c.max_lines} min={1} max={8} onsave={v => setValue("live_captions.max_lines", v)} /></Row>
      <Row label="Width (px)"><NumberInput value={c.width_px} min={280} max={1200} onsave={v => setValue("live_captions.width_px", v)} /></Row>
      <Row label="Always on top"><Toggle label="Always on top" checked={c.always_on_top} onchange={v => setValue("live_captions.always_on_top", v)} /></Row>
      <Row label="Shadow"><Toggle label="Caption shadow" checked={c.shadow} onchange={v => setValue("live_captions.shadow", v)} /></Row>
      <Row label="Menu bar characters"><NumberInput value={c.menu_bar_max_chars} min={10} max={80} onsave={v => setValue("live_captions.menu_bar_max_chars", v)} /></Row>
      <Row label="Overlay visibility" hint="Assign an optional shortcut in Keyboard shortcuts."><button class="btn btn-xs btn-ghost" onclick={() => invoke("toggle_caption_overlay")}>Show / hide</button></Row>
    {/if}
  </Group>
{/if}
