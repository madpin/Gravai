<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "../lib/tauri";

  let profiles = $state<Record<string, any>>({});
  let activeId = $state<string | null>(null);

  onMount(load);

  async function load() {
    try {
      const store: any = await invoke("get_profiles");
      profiles = store.profiles || {};
      activeId = store.active_profile_id;
    } catch (_) {}
  }

  async function activate(id: string) {
    try { await invoke("activate_profile", { profileId: id }); activeId = id; } catch (_) {}
  }
</script>

<div class="page-header"><h2>Profiles</h2></div>
<p class="page-desc">Profiles bundle settings for different contexts. Switch between work, podcast, and minimal modes.</p>

<div class="card-grid">
  {#each Object.entries(profiles) as [id, p]}
    <div class="card" class:active-card={activeId === id}>
      <div class="card-header">{p.name} {#if activeId === id}<span class="card-tag" style="background:var(--accent-glow);color:var(--accent)">Active</span>{/if}</div>
      <div class="card-body">
        <p>{p.description}</p>
        <div style="margin-top:6px">
          {#if p.preset_id}<span class="card-tag">Preset: {p.preset_id}</span>{/if}
          {#if p.transcription_model}<span class="card-tag">Model: {p.transcription_model}</span>{/if}
          {#if p.transcription_language}<span class="card-tag">Lang: {p.transcription_language}</span>{/if}
          {#if p.diarization_enabled != null}<span class="card-tag">Diarization: {p.diarization_enabled ? "On" : "Off"}</span>{/if}
          {#if p.llm_provider}<span class="card-tag">LLM: {p.llm_provider}</span>{/if}
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
