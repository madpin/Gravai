<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "../lib/tauri";

  let presets = $state<Record<string, any>>({});
  let activeId = $state<string | null>(null);

  onMount(load);

  async function load() {
    try {
      const store: any = await invoke("get_presets");
      presets = store.presets || {};
      activeId = store.active_preset_id;
    } catch (_) {}
  }

  async function activate(id: string) {
    try { await invoke("activate_preset", { presetId: id }); activeId = id; } catch (_) {}
  }
</script>

<div class="page-header"><h2>Capture Presets</h2></div>
<p class="page-desc">Switch audio configurations in one click. Presets control sources, quality, and export format.</p>

<div class="card-grid">
  {#each Object.entries(presets) as [id, p]}
    <div class="card" class:active-card={activeId === id}>
      <div class="card-header">{p.name} {#if activeId === id}<span class="card-tag" style="background:var(--accent-glow);color:var(--accent)">Active</span>{/if}</div>
      <div class="card-body">
        <p>{p.description}</p>
        <div style="margin-top:6px">
          <span class="card-tag">{p.mic_enabled ? "🎤 Mic" : "🎤 Off"}</span>
          <span class="card-tag">{p.sys_enabled ? "💻 System" : "💻 Off"}</span>
          <span class="card-tag">{p.sample_rate / 1000}kHz</span>
          <span class="card-tag">{p.bit_depth}-bit</span>
          <span class="card-tag">{p.channels === 1 ? "Mono" : "Stereo"}</span>
          <span class="card-tag">{p.export_format}</span>
        </div>
      </div>
      <div class="card-actions">
        <button class="btn btn-xs btn-accent" onclick={() => activate(id)} disabled={activeId === id}>
          {activeId === id ? "Active" : "Activate"}
        </button>
      </div>
    </div>
  {/each}
</div>
