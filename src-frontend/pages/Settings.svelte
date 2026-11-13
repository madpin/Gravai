<script lang="ts">
  import { onMount } from "svelte";
  import { settingsSection, type SettingsSection } from "../lib/store";
  import { config, loadConfig } from "../lib/config";
  import Icon from "../components/Icon.svelte";
  import Capture from "./settings/Capture.svelte";
  import Speech from "./settings/Speech.svelte";
  import Assistant from "./settings/Assistant.svelte";
  import Search from "./settings/Search.svelte";
  import Sessions from "./settings/Sessions.svelte";
  import Library from "./settings/Library.svelte";
  import AppSection from "./settings/AppSection.svelte";

  // Every setting lives in exactly one of these sections.
  const SECTIONS: Array<{ id: SettingsSection; label: string; icon: string; blurb: string }> = [
    { id: "capture", label: "Capture", icon: "microphone", blurb: "What gets recorded and how the audio is saved." },
    { id: "speech", label: "Speech", icon: "message-circle", blurb: "How speech becomes a transcript, and what else is analysed." },
    { id: "assistant", label: "Assistant", icon: "bot", blurb: "The model that writes summaries, answers questions, and corrects transcripts." },
    { id: "search", label: "Search", icon: "books", blurb: "How recordings are found by meaning." },
    { id: "sessions", label: "Sessions", icon: "calendar", blurb: "Meetings, automatic start and stop, and what happens after a recording." },
    { id: "library", label: "Library", icon: "folder", blurb: "Knowledge, recordings on disk, and downloaded models." },
    { id: "app", label: "App", icon: "settings", blurb: "Shortcuts, startup, updates, health, and the settings file." },
  ];

  let current = $derived(SECTIONS.find(s => s.id === $settingsSection) ?? SECTIONS[0]);

  onMount(() => { if (!$config) loadConfig(); });
</script>

<div class="settings">
  <nav class="sections" aria-label="Settings sections">
    <h2>Settings</h2>
    {#each SECTIONS as s}
      <button class="section" class:active={s.id === current.id} aria-current={s.id === current.id ? "page" : undefined} onclick={() => settingsSection.set(s.id)}>
        <Icon name={s.icon} size={15} />
        <span>{s.label}</span>
      </button>
    {/each}
  </nav>

  <div class="pane">
    <header>
      <h1>{current.label}</h1>
      <p>{current.blurb}</p>
    </header>
    {#key current.id}
      <div class="body">
        {#if current.id === "capture"}<Capture />
        {:else if current.id === "speech"}<Speech />
        {:else if current.id === "assistant"}<Assistant />
        {:else if current.id === "search"}<Search />
        {:else if current.id === "sessions"}<Sessions />
        {:else if current.id === "library"}<Library />
        {:else}<AppSection />{/if}
      </div>
    {/key}
  </div>
</div>

<style>
  /* The section list stays put; only the pane on the right scrolls. */
  .settings { display: flex; gap: 28px; flex: 1; min-height: 0; margin: 0 -28px -24px 0; }
  .sections {
    width: 176px; flex-shrink: 0; overflow-y: auto;
    display: flex; flex-direction: column; gap: 2px;
  }
  .sections h2 { margin: 0 0 12px 10px; font-size: 18px; font-weight: 700; color: var(--text-primary); }
  .section {
    display: flex; align-items: center; gap: 10px; padding: 7px 10px; border-radius: 8px;
    background: none; border: none; color: var(--text-secondary); font: inherit; font-size: 13px;
    cursor: pointer; text-align: left;
  }
  .section:hover { background: var(--bg-secondary); color: var(--text-primary); }
  .section.active { background: var(--bg-elevated); color: var(--text-primary); font-weight: 600; }
  .section.active :global(svg) { color: var(--accent); }
  .pane {
    flex: 1; min-width: 0; overflow-y: auto; overscroll-behavior: contain;
    display: flex; flex-direction: column; gap: 20px; padding: 0 28px 40px 2px;
  }
  .pane > * { max-width: 760px; width: 100%; }
  header h1 { margin: 2px 0 4px; font-size: 20px; font-weight: 700; color: var(--text-primary); }
  header p { margin: 0; font-size: 13px; color: var(--text-secondary); }
  .body { display: flex; flex-direction: column; gap: 22px; }
</style>
