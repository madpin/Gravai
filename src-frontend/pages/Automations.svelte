<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "../lib/tauri";

  let automations = $state<Record<string, any>>({});

  onMount(load);

  async function load() {
    try {
      const store: any = await invoke("get_automations");
      automations = store.automations || {};
    } catch (_) {}
  }

  async function toggle(id: string, enabled: boolean) {
    try {
      await invoke("toggle_automation", { automationId: id, enabled });
      if (automations[id]) automations[id] = { ...automations[id], enabled };
    } catch (_) {}
  }

  const AUTOMATION_ORDER = [
    "auto-meeting-record",
    "zoom-auto-stop",
    "silence-auto-stop",
    "auto-export-on-stop",
  ];

  let ordered = $derived(
    Object.entries(automations).sort((a, b) => {
      const ia = AUTOMATION_ORDER.indexOf(a[0]);
      const ib = AUTOMATION_ORDER.indexOf(b[0]);
      return (ia === -1 ? 99 : ia) - (ib === -1 ? 99 : ib);
    }),
  );

  function triggerLabel(trigger: any): string {
    if (typeof trigger === "string") {
      const map: Record<string, string> = {
        meeting_detected: "A meeting starts",
        meeting_ended: "The meeting ends",
        audio_silent: "Mic and system audio stay quiet",
        session_started: "Recording starts",
        session_ended: "Recording stops",
        calendar_event_starting: "A calendar event is starting",
      };
      return map[trigger] ?? trigger.replace(/_/g, " ");
    }
    const app =
      trigger?.meeting_app_detected?.app_name
      ?? trigger?.MeetingAppDetected?.app_name;
    if (app && (trigger?.meeting_app_detected || trigger?.MeetingAppDetected)) {
      return `${app} meeting starts`;
    }
    const ended =
      trigger?.meeting_app_ended?.app_name
      ?? trigger?.MeetingAppEnded?.app_name;
    if (ended) return `${ended} meeting ends`;
    const time = trigger?.time_of_day ?? trigger?.TimeOfDay;
    if (time) return `Time: ${time.hour}:${String(time.minute).padStart(2, "0")}`;
    const foreground = trigger?.app_foregrounded?.app_name ?? trigger?.AppForegrounded?.app_name;
    if (foreground) return `${foreground} comes to the front`;
    return JSON.stringify(trigger);
  }

  function actionLabels(actions: any[]): string[] {
    return actions.map(a => {
      if (a === "start_recording") return "Start recording";
      if (a === "stop_recording") return "Stop recording";
      if (typeof a === "string") return a.replace(/_/g, " ");
      const key = Object.keys(a)[0];
      if (!key) return "?";
      const val = a[key];
      if (val?.profile_id) {
        const name = val.profile_id === "meeting" ? "Meeting" : val.profile_id;
        return `Switch to the ${name} profile`;
      }
      if (val?.preset_id) return `Switch to the ${val.preset_id} preset`;
      if (val?.message) return `Notify: ${val.message}`;
      if (val?.format) return `Save a ${val.format} transcript`;
      if (typeof val === "string") return `${key.replace(/_/g, " ")}: ${val}`;
      return key.replace(/_/g, " ");
    });
  }
</script>

<div class="page-header"><h2>Automations</h2></div>
<p class="page-desc">Meeting start and meeting end are a pair. Silence is a separate safety net, with its own countdown. Export writes the transcript when a recording stops.</p>

<div class="automation-grid">
  {#each ordered as [id, a]}
    <div class="automation-card" class:enabled={a.enabled}>
      <div class="automation-card-top">
        <div class="automation-name">
          {a.name}
          {#if a.description}<div class="automation-desc">{a.description}</div>{/if}
        </div>
        <label class="automation-toggle">
          <input type="checkbox" checked={a.enabled} onchange={() => toggle(id, !a.enabled)} />
          <span class="automation-toggle-track">
            <span class="automation-toggle-thumb"></span>
          </span>
          <span class="automation-toggle-label">{a.enabled ? "On" : "Off"}</span>
        </label>
      </div>
      <div class="automation-card-body">
        <div class="automation-detail">
          <span class="automation-detail-key">Trigger</span>
          <span class="automation-detail-val">{triggerLabel(a.trigger)}</span>
        </div>
        <div class="automation-detail">
          <span class="automation-detail-key">Actions</span>
          <span class="automation-detail-val">{actionLabels(a.actions).join(" → ")}</span>
        </div>
        {#if a.last_run}
          <div class="automation-last-run">
            Last run: {new Date(a.last_run).toLocaleString()} · {a.run_count}×
          </div>
        {:else}
          <div class="automation-last-run">Never run</div>
        {/if}
      </div>
    </div>
  {/each}
</div>

<style>
  .automation-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
    gap: 14px;
  }
  .automation-card {
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    overflow: hidden;
    transition: border-color 0.2s;
  }
  .automation-card.enabled {
    border-color: var(--accent-dim);
    box-shadow: 0 0 0 1px var(--accent-glow);
  }
  .automation-card-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 18px 12px;
    gap: 12px;
  }
  .automation-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    line-height: 1.3;
    flex: 1;
  }
  .automation-desc {
    margin-top: 4px;
    font-size: 12px;
    font-weight: 400;
    color: var(--text-secondary);
    line-height: 1.4;
  }
  /* Big pill toggle */
  .automation-toggle {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    user-select: none;
    flex-shrink: 0;
  }
  .automation-toggle input { display: none; }
  .automation-toggle-track {
    position: relative;
    width: 44px;
    height: 26px;
    border-radius: 13px;
    background: var(--bg-base);
    border: 1px solid var(--border);
    transition: background 0.2s, border-color 0.2s;
    flex-shrink: 0;
  }
  .automation-toggle input:checked ~ .automation-toggle-track {
    background: var(--accent);
    border-color: var(--accent);
  }
  .automation-toggle-thumb {
    position: absolute;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: var(--text-secondary);
    top: 2px;
    left: 2px;
    transition: transform 0.2s, background 0.2s;
    box-shadow: 0 1px 3px rgba(0,0,0,0.3);
  }
  .automation-toggle input:checked ~ .automation-toggle-track .automation-toggle-thumb {
    transform: translateX(18px);
    background: white;
  }
  .automation-toggle-label {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-tertiary);
    min-width: 22px;
  }
  .automation-toggle input:checked ~ .automation-toggle-label {
    color: var(--accent);
  }
  .automation-card-body {
    padding: 0 18px 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    border-top: 1px solid var(--border-subtle);
    padding-top: 12px;
  }
  .automation-detail {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .automation-detail-key {
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.8px;
    color: var(--text-tertiary);
  }
  .automation-detail-val {
    font-size: 13px;
    color: var(--text-primary);
    line-height: 1.4;
  }
  .automation-last-run {
    font-size: 11px;
    color: var(--text-tertiary);
    margin-top: 4px;
  }
</style>
