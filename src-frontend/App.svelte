<script lang="ts">
  import { currentPage, healthStatus, addAlert, modelDownloading, llmStatus, navigate } from "./lib/store";
  import { loadConfig } from "./lib/config";
  import { scanIncomplete, recoveryOpen } from "./lib/recovery";
  import RecoveryDialog from "./components/RecoveryDialog.svelte";
  import { invoke, listen } from "./lib/tauri";
  import { onMount, onDestroy } from "svelte";
  import Onboarding from "./components/Onboarding.svelte";
  import AlertBar from "./components/AlertBar.svelte";
  import StatusBar from "./components/StatusBar.svelte";
  import Icon from "./components/Icon.svelte";

  import Recording from "./pages/Recording.svelte";
  import Archive from "./pages/Archive.svelte";
  import Chat from "./pages/Chat.svelte";
  import Settings from "./pages/Settings.svelte";

  const mainPages = [
    { id: "recording", label: "Record", icon: "record" },
    { id: "archive", label: "Archive", icon: "archive" },
    { id: "chat", label: "Ask", icon: "chat" },
  ];


  let showOnboarding = $state(false);
  let sidebarCollapsed = $state(localStorage.getItem("gravai_sidebar_collapsed") === "true");

  function toggleSidebar() {
    sidebarCollapsed = !sidebarCollapsed;
    localStorage.setItem("gravai_sidebar_collapsed", String(sidebarCollapsed));
  }
  let unlistenNavigate: (() => void) | null = null;
  let unlistenUpdate: (() => void) | null = null;
  let unlistenDownload: (() => void) | null = null;
  let unlistenLlmStatus: (() => void) | null = null;

  onMount(async () => {
    loadConfig();
    if (!localStorage.getItem("gravai_onboarded")) {
      showOnboarding = true;
    } else {
      // Recordings Gravai closed on without stopping: offer to repair them.
      scanIncomplete().then((found) => { if (found.length) recoveryOpen.set(true); });
    }
    try {
      const report: any = await invoke("get_health_report");
      healthStatus.set(report.overall);
      for (const check of report.checks ?? []) {
        if (check.name === "microphone_permission" && check.status === "error") {
          addAlert({
            level: "error",
            message: "Microphone access denied — Gravai cannot record audio.",
            actions: [{ label: "Open Settings", handler: () => invoke("open_privacy_settings", { permission: "microphone" }) }],
            dismissable: true,
          });
        }
        if (check.name === "screen_recording_permission" && check.status === "error") {
          addAlert({
            level: "error",
            message: "Screen Recording access denied — system audio capture unavailable.",
            actions: [{ label: "Open Settings", handler: () => invoke("open_privacy_settings", { permission: "screen_recording" }) }],
            dismissable: true,
          });
        } else if (check.name === "screen_recording_permission" && check.status === "warn") {
          addAlert({
            level: "warning",
            message: "Screen Recording not granted — enable it to capture system audio.",
            actions: [{ label: "Open Settings", handler: () => invoke("open_privacy_settings", { permission: "screen_recording" }) }],
            dismissable: true,
          });
        }
      }
    } catch (_) {}
    unlistenNavigate = await listen("gravai:navigate", (e: any) => {
      if (e.payload) navigate(e.payload);
    });
    unlistenUpdate = await listen("gravai:update-available", (e: any) => {
      const v = e.payload?.version;
      addAlert({
        level: "info",
        message: `Gravai v${v} is available — go to Settings to update`,
        actions: [{ label: "Settings", handler: () => navigate("settings:app") }],
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

    // Local LLM engine lifecycle. We mirror it into a global store so any
    // page (Recording, Chat, Archive…) can show progress, and we promote the
    // (rare) error case to an alert so the user notices.
    unlistenLlmStatus = await listen("gravai:llm-status", (e: any) => {
      const d = e.payload;
      if (!d?.state) return;
      llmStatus.update(prev => {
        // Set started_at when transitioning into any active load state.
        const isActiveLoad = d.state === "loading"
          || d.state === "first_run"
          || d.state === "progress"
          || d.state === "summarizing";
        const wasActive = prev.state === "loading"
          || prev.state === "first_run"
          || prev.state === "progress"
          || prev.state === "summarizing";
        const isLoadStart = isActiveLoad && !wasActive;

        // Preserve the high-level state across "progress" tick events so the
        // UI keeps showing first-run vs loading framing. "summarizing" is
        // its own framing so it overrides previous active states cleanly.
        let nextState = d.state;
        if (d.state === "progress" && wasActive) {
          nextState = prev.state;
        }

        return {
          state: nextState,
          model_id: d.model_id ?? prev.model_id,
          message: d.message ?? prev.message,
          progress: typeof d.progress === "number" ? d.progress : prev.progress,
          phase: d.phase ?? prev.phase,
          eta_seconds: typeof d.eta_seconds === "number" ? d.eta_seconds : prev.eta_seconds,
          started_at: isLoadStart
            ? Date.now()
            : (d.state === "ready" || d.state === "idle" || d.state === "unloaded" || d.state === "error"
                ? null
                : prev.started_at),
        };
      });
      if (d.state === "error" && d.message) {
        addAlert({
          level: "error",
          message: `Local LLM error: ${d.message}`,
          actions: [{ label: "Assistant settings", handler: () => navigate("settings:assistant") }],
          dismissable: true,
        });
      }
    });
  });

  onDestroy(() => { unlistenNavigate?.(); unlistenUpdate?.(); unlistenDownload?.(); unlistenLlmStatus?.(); });

  function setPage(id: string) {
    currentPage.set(id);
  }
</script>

{#if showOnboarding}
  <Onboarding onComplete={() => showOnboarding = false} />
{/if}

<div class="app">
  <div class="app-body">
  <nav class="sidebar" class:collapsed={sidebarCollapsed}>
    <div class="sidebar-header">
      <img src="/icon.png" alt="Gravai" class="app-icon" title={sidebarCollapsed ? "Gravai" : undefined} />
      {#if !sidebarCollapsed}
        <div class="sidebar-header-text">
          <h1>Gravai</h1>
          <span class="subtitle">Audio Intelligence</span>
        </div>
      {/if}
    </div>
    <ul class="nav-list">
      {#each mainPages as page}
        <!-- svelte-ignore a11y_no_noninteractive_element_to_interactive_role -->
        <li
          class="nav-item"
          class:active={$currentPage === page.id}
          role="button"
          tabindex="0"
          title={sidebarCollapsed ? page.label : undefined}
          onclick={() => setPage(page.id)}
          onkeydown={(e) => { if (e.key === "Enter") setPage(page.id); }}
        >
          <span class="nav-icon"><Icon name={page.icon} size={18} /></span>
          {#if !sidebarCollapsed}{page.label}{/if}
        </li>
      {/each}

      <li class="nav-divider" aria-hidden="true"></li>
      <!-- svelte-ignore a11y_no_noninteractive_element_to_interactive_role -->
      <li
        class="nav-item"
        class:active={$currentPage === "settings"}
        role="button"
        tabindex="0"
        title={sidebarCollapsed ? "Settings" : undefined}
        onclick={() => setPage("settings")}
        onkeydown={(e) => { if (e.key === "Enter") setPage("settings"); }}
      >
        <span class="nav-icon"><Icon name="settings" size={18} /></span>
        {#if !sidebarCollapsed}Settings{/if}
      </li>
    </ul>
    <div class="sidebar-collapse-btn-wrap">
      <button
        class="sidebar-collapse-btn"
        onclick={toggleSidebar}
        title={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
        aria-label={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
      >
        <Icon name={sidebarCollapsed ? "chevron-right" : "chevron-left"} size={14} />
      </button>
    </div>
  </nav>

  <main class="content">
    <AlertBar />
    {#if $currentPage === "recording"}
      <Recording />
    {:else if $currentPage === "archive"}
      <Archive />
    {:else if $currentPage === "chat"}
      <Chat />
    {:else if $currentPage === "settings"}
      <Settings />
    {/if}
  </main>
  </div>
  <StatusBar />
  <RecoveryDialog />
</div>
