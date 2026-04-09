<script lang="ts">
  import { currentPage, healthStatus, addAlert, modelDownloading } from "./lib/store";
  import { invoke, listen } from "./lib/tauri";
  import { onMount, onDestroy } from "svelte";
  import Onboarding from "./components/Onboarding.svelte";
  import AlertBar from "./components/AlertBar.svelte";
  import StatusBar from "./components/StatusBar.svelte";

  import Recording from "./pages/Recording.svelte";
  import Archive from "./pages/Archive.svelte";
  import Presets from "./pages/Presets.svelte";
  import Profiles from "./pages/Profiles.svelte";
  import Shortcuts from "./pages/Shortcuts.svelte";
  import Automations from "./pages/Automations.svelte";
  import Chat from "./pages/Chat.svelte";
  import Storage from "./pages/Storage.svelte";
  import Models from "./pages/Models.svelte";
  import Settings from "./pages/Settings.svelte";

  const mainPages = [
    { id: "recording", label: "Recording", icon: "⏺" },
    { id: "archive", label: "Archive", icon: "📦" },
    { id: "chat", label: "Ask Gravai", icon: "💬" },
  ];

  const configPages = [
    { id: "presets", label: "Presets", icon: "🎛️" },
    { id: "profiles", label: "Profiles", icon: "👤" },
    { id: "models", label: "Models", icon: "🧠" },
    { id: "shortcuts", label: "Shortcuts", icon: "⌨️" },
    { id: "automations", label: "Automations", icon: "⚡" },
    { id: "storage", label: "Storage", icon: "💿" },
    { id: "settings", label: "System", icon: "⚙️" },
  ];

  let showOnboarding = $state(false);
  let settingsOpen = $state(false);
  let unlistenNavigate: (() => void) | null = null;
  let unlistenUpdate: (() => void) | null = null;
  let unlistenDownload: (() => void) | null = null;

  onMount(async () => {
    if (!localStorage.getItem("gravai_onboarded")) {
      showOnboarding = true;
    }
    try {
      const report: any = await invoke("get_health_report");
      healthStatus.set(report.overall);
    } catch (_) {}
    unlistenNavigate = await listen("gravai:navigate", (e: any) => {
      if (e.payload) currentPage.set(e.payload);
    });
    unlistenUpdate = await listen("gravai:update-available", (e: any) => {
      const v = e.payload?.version;
      addAlert({
        level: "info",
        message: `Gravai v${v} is available — go to Settings to update`,
        actions: [{ label: "Settings", handler: () => currentPage.set("settings") }],
        dismissable: true,
      });
    });
    unlistenDownload = await listen("gravai:model-download", (e: any) => {
      const d = e.payload?.data || e.payload;
      if (!d?.model_id) return;
      modelDownloading.update(cur => ({ ...cur, [d.model_id]: { progress: d.progress || 0, status: d.status || "" } }));
      if (d.status === "complete" || d.status === "error") {
        setTimeout(() => {
          modelDownloading.update(cur => { const { [d.model_id]: _, ...rest } = cur; return rest; });
        }, 1500);
      }
    });
  });

  onDestroy(() => { unlistenNavigate?.(); unlistenUpdate?.(); unlistenDownload?.(); });

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
  <div class="app-body">
  <nav class="sidebar">
    <div class="sidebar-header">
      <img src="/icon.png" alt="Gravai" class="app-icon" />
      <div class="sidebar-header-text">
        <h1>Gravai</h1>
        <span class="subtitle">Audio Intelligence</span>
      </div>
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
  </nav>

  <main class="content">
    <AlertBar />
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
    {:else if $currentPage === "models"}
      <Models />
    {:else if $currentPage === "settings"}
      <Settings />
    {/if}
  </main>
  </div>
  <StatusBar />
</div>
