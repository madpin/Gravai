<script lang="ts">
  import { currentPage, healthStatus } from "./lib/store";
  import { invoke } from "./lib/tauri";
  import { onMount } from "svelte";
  import Onboarding from "./components/Onboarding.svelte";

  import Recording from "./pages/Recording.svelte";
  import Archive from "./pages/Archive.svelte";
  import Presets from "./pages/Presets.svelte";
  import Profiles from "./pages/Profiles.svelte";
  import Shortcuts from "./pages/Shortcuts.svelte";
  import Automations from "./pages/Automations.svelte";
  import Chat from "./pages/Chat.svelte";
  import Storage from "./pages/Storage.svelte";
  import Settings from "./pages/Settings.svelte";

  const mainPages = [
    { id: "recording", label: "Recording", icon: "⏺" },
    { id: "archive", label: "Archive", icon: "📦" },
    { id: "chat", label: "Ask Gravai", icon: "💬" },
  ];

  const configPages = [
    { id: "presets", label: "Presets", icon: "🎛️" },
    { id: "profiles", label: "Profiles", icon: "👤" },
    { id: "shortcuts", label: "Shortcuts", icon: "⌨️" },
    { id: "automations", label: "Automations", icon: "⚡" },
    { id: "storage", label: "Storage", icon: "💿" },
    { id: "settings", label: "System", icon: "⚙️" },
  ];

  let health = $state("ok");
  let showOnboarding = $state(false);
  let settingsOpen = $state(false);
  healthStatus.subscribe((v) => (health = v));

  onMount(async () => {
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

  // Auto-expand settings section when a config page is active
  $effect(() => {
    if (configPages.some(p => p.id === $currentPage)) {
      settingsOpen = true;
    }
  });
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
      {#each mainPages as page}
        <!-- svelte-ignore a11y_no_noninteractive_element_to_interactive_role -->
        <li
          class="nav-item"
          class:active={$currentPage === page.id}
          role="button"
          tabindex="0"
          onclick={() => setPage(page.id)}
          onkeydown={(e) => { if (e.key === "Enter") setPage(page.id); }}
        >
          <span class="nav-icon">{page.icon}</span>
          {page.label}
        </li>
      {/each}

      <!-- Settings group -->
      <li class="nav-section">
        <button class="nav-section-toggle" onclick={() => settingsOpen = !settingsOpen}>
          <span class="nav-icon">⚙️</span>
          Settings
          <span class="nav-section-arrow" class:open={settingsOpen}>{settingsOpen ? "▾" : "▸"}</span>
        </button>
        {#if settingsOpen}
          <ul class="nav-sublist">
            {#each configPages as page}
              <!-- svelte-ignore a11y_no_noninteractive_element_to_interactive_role -->
              <li
                class="nav-item sub"
                class:active={$currentPage === page.id}
                role="button"
                tabindex="0"
                onclick={() => setPage(page.id)}
                onkeydown={(e) => { if (e.key === "Enter") setPage(page.id); }}
              >
                <span class="nav-icon">{page.icon}</span>
                {page.label}
              </li>
            {/each}
          </ul>
        {/if}
      </li>
    </ul>
    <div class="sidebar-footer">
      <div class="health-dot" class:green={health === "ok"} class:yellow={health === "warn"} class:red={health === "error"}></div>
      <span class="version">v1.0.0</span>
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
    {:else if $currentPage === "storage"}
      <Storage />
    {:else if $currentPage === "settings"}
      <Settings />
    {/if}
  </main>
</div>
