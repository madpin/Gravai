<script lang="ts">
  import { invoke } from "../lib/tauri";
  import { onMount } from "svelte";
  import Icon from "./Icon.svelte";

  let { onComplete }: { onComplete: () => void } = $props();

  let step = $state(0);
  let healthChecks = $state<any[]>([]);
  let perfInfo = $state<any>(null);
  let deviceCount = $state(0);
  let checking = $state(false);

  const steps = [
    {
      title: "Welcome to Gravai",
      desc: "Audio Capture & AI Meeting Intelligence.\nEverything runs locally on your Mac — your data never leaves your machine.",
      icon: "microphone"
    },
    {
      title: "How It Works",
      desc: "1. Select your audio sources (microphone, system audio)\n2. Click Record to start capturing\n3. Gravai transcribes in real-time using on-device AI\n4. Get summaries, action items, and searchable archives",
      icon: "clipboard"
    },
    {
      title: "Permissions",
      desc: "• Microphone — required for voice recording\n• Screen Recording — only requested when you record system audio\n• Calendar — optional, for auto-naming sessions from events\n\nNo permission is requested until you use the feature.",
      icon: "lock"
    },
    {
      title: "System Check",
      desc: "Checking your system configuration...",
      icon: "settings"
    },
    {
      title: "You're All Set!",
      desc: "Start recording your first meeting.\nTip: Check Settings to customize transcription language, AI model, and export preferences.",
      icon: "rocket"
    },
  ];

  async function runSystemCheck() {
    checking = true;
    try {
      const report: any = await invoke("get_health_report");
      healthChecks = report.checks || [];
    } catch (_) {}
    try {
      const devices: any[] = await invoke("list_audio_devices");
      deviceCount = devices.length;
    } catch (_) {}
    try {
      perfInfo = await invoke("get_perf_snapshot");
    } catch (_) {}
    checking = false;
  }

  function next() {
    if (step < steps.length - 1) {
      step++;
      // Auto-run system check when arriving at the check step
      if (step === 3) runSystemCheck();
    } else {
      finish();
    }
  }
  function prev() { if (step > 0) step--; }
  function finish() {
    localStorage.setItem("gravai_onboarded", "1");
    onComplete();
  }
</script>

<div class="onboarding-overlay">
  <div class="onboarding-card">
    <div class="step-indicator">
      {#each steps as _, i}
        <div class="step-dot" class:active={i <= step}></div>
      {/each}
    </div>

    <div class="onboarding-step-icon"><Icon name={steps[step].icon} size={48}/></div>
    <h2>{steps[step].title}</h2>
    <p style="white-space:pre-line">{steps[step].desc}</p>

    {#if step === 3}
      <div class="onboarding-checks">
        {#if checking}
          <div class="onboarding-check-loading">Running checks...</div>
        {:else if healthChecks.length > 0}
          {#each healthChecks as check}
            <div class="onboarding-check-row">
              <span class="onboarding-check-icon">
                {#if check.status === 'ok'}<Icon name="check-circle" size={16}/>{:else if check.status === 'warn'}<Icon name="alert-triangle" size={16}/>{:else}<Icon name="x-circle" size={16}/>{/if}
              </span>
              <div class="onboarding-check-info">
                <span class="onboarding-check-name">{check.name}</span>
                <span class="onboarding-check-msg">{check.message}</span>
              </div>
            </div>
          {/each}
          {#if perfInfo}
            <div class="onboarding-check-row">
              <span class="onboarding-check-icon"><Icon name="save" size={16}/></span>
              <div class="onboarding-check-info">
                <span class="onboarding-check-name">Memory</span>
                <span class="onboarding-check-msg">{perfInfo.rss_mb.toFixed(0)} MB used / {perfInfo.total_memory_gb.toFixed(0)} GB total</span>
              </div>
            </div>
          {/if}
          {#if deviceCount > 0}
            <div class="onboarding-check-row">
              <span class="onboarding-check-icon"><Icon name="headphones" size={16}/></span>
              <div class="onboarding-check-info">
                <span class="onboarding-check-name">Audio Devices</span>
                <span class="onboarding-check-msg">{deviceCount} input device(s) detected</span>
              </div>
            </div>
          {/if}
        {:else}
          <div class="onboarding-check-loading">Click Next to run system check</div>
        {/if}
      </div>
    {/if}

    <div class="onboarding-actions">
      {#if step > 0}
        <button class="btn btn-ghost" onclick={prev}>Back</button>
      {/if}
      <button class="btn btn-accent" onclick={next}>
        {step === steps.length - 1 ? "Get Started" : "Next"}
      </button>
      {#if step < steps.length - 1}
        <button class="btn btn-ghost" style="font-size:11px" onclick={finish}>Skip</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .onboarding-checks {
    text-align: left;
    margin: 16px 0 8px;
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
  }
  .onboarding-check-row {
    display: flex; align-items: flex-start; gap: 10px;
    padding: 8px 14px;
    border-bottom: 1px solid var(--border-subtle);
  }
  .onboarding-check-row:last-child { border-bottom: none; }
  .onboarding-check-icon { font-size: 16px; flex-shrink: 0; margin-top: 1px; }
  .onboarding-check-info { display: flex; flex-direction: column; gap: 1px; }
  .onboarding-check-name { font-size: 12px; font-weight: 600; color: var(--text-primary); text-transform: capitalize; }
  .onboarding-check-msg { font-size: 11px; color: var(--text-secondary); line-height: 1.4; }
  .onboarding-check-loading { padding: 16px; text-align: center; color: var(--text-tertiary); font-size: 12px; }
  .onboarding-actions { margin-top: 20px; display: flex; justify-content: center; gap: 8px; }
</style>
