<script lang="ts">
  import { currentPage, healthStatus } from "./lib/store";
  import { invoke, listen } from "./lib/tauri";
  import { onMount } from "svelte";
  import Onboarding from "./components/Onboarding.svelte";

  import Recording from "./pages/Recording.svelte";
  import Archive from "./pages/Archive.svelte";
  import Presets from "./pages/Presets.svelte";
  import Profiles from "./pages/Profiles.svelte";
  import Shortcuts from "./pages/Shortcuts.svelte";
  import Automations from "./pages/Automations.svelte";
  import Chat from "./pages/Chat.svelte";
  import Settings from "./pages/Settings.svelte";

  const pages = [
    { id: "recording", label: "Recording", icon: "circle" },
    { id: "archive", label: "Archive", icon: "archive" },
    { id: "chat", label: "Ask Gravai", icon: "chat" },
    { id: "presets", label: "Presets", icon: "sliders" },
    { id: "profiles", label: "Profiles", icon: "user" },
    { id: "shortcuts", label: "Shortcuts", icon: "keyboard" },
    { id: "automations", label: "Automations", icon: "zap" },
    { id: "settings", label: "Settings", icon: "gear" },
  ];

  let health = $state("ok");
  let showOnboarding = $state(false);
  healthStatus.subscribe((v) => (health = v));

  onMount(async () => {
    // Show onboarding on first launch
    if (!localStorage.getItem("gravai_onboarded")) {
      showOnboarding = true;
    }
    try {
      const report: any = await invoke("get_health_report");
      healthStatus.set(report.overall);
    } catch (_) {}
  });

  function setPage(id: string) {
    currentPage.set(id);
  }
</script>

{#if showOnboarding}
  <Onboarding onComplete={() => showOnboarding = false} />
{/if}

<div class="app">
  <nav class="sidebar">
    <div class="sidebar-header">
      <h1>Gravai</h1>
      <span class="subtitle">Audio Intelligence</span>
    </div>
    <ul class="nav-list">
      {#each pages as page}
        <!-- svelte-ignore a11y_no_noninteractive_element_to_interactive_role -->
        <li
          class="nav-item"
          class:active={$currentPage === page.id}
          role="button"
          tabindex="0"
          onclick={() => setPage(page.id)}
          onkeydown={(e) => { if (e.key === "Enter") setPage(page.id); }}
        >
          <span class="nav-icon">{page.icon === "circle" ? "⏺" : page.icon === "archive" ? "📦" : page.icon === "chat" ? "💬" : page.icon === "sliders" ? "🎛️" : page.icon === "user" ? "👤" : page.icon === "keyboard" ? "⌨️" : page.icon === "zap" ? "⚡" : "⚙️"}</span>
          {page.label}
        </li>
      {/each}
    </ul>
    <div class="sidebar-footer">
      <div class="health-dot" class:green={health === "ok"} class:yellow={health === "warn"} class:red={health === "error"}></div>
      <span class="version">v0.5.0-beta</span>
    </div>
  </nav>

  <main class="content">
    {#if $currentPage === "recording"}
      <Recording />
    {:else if $currentPage === "archive"}
      <Archive />
    {:else if $currentPage === "chat"}
      <Chat />
    {:else if $currentPage === "presets"}
      <Presets />
    {:else if $currentPage === "profiles"}
      <Profiles />
    {:else if $currentPage === "shortcuts"}
      <Shortcuts />
    {:else if $currentPage === "automations"}
      <Automations />
    {:else if $currentPage === "settings"}
      <Settings />
    {/if}
  </main>
</div>
