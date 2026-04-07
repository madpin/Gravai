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

  function triggerLabel(trigger: any): string {
    if (typeof trigger === "string") {
      const map: Record<string, string> = {
        meeting_detected: "Any meeting app detected",
        session_started: "Session started",
        session_ended: "Session ended",
        calendar_event_starting: "Calendar event starting",
      };
      return map[trigger] ?? trigger.replace(/_/g, " ");
    }
    if (trigger?.TimeOfDay) return `Time: ${trigger.TimeOfDay.hour}:${String(trigger.TimeOfDay.minute).padStart(2, "0")}`;
    if (trigger?.AppForegrounded) return `App foregrounded: ${trigger.AppForegrounded.app_name}`;
    if (trigger?.MeetingAppDetected) return `${trigger.MeetingAppDetected.app_name} meeting starts`;
    if (trigger?.MeetingAppEnded) return `${trigger.MeetingAppEnded.app_name} meeting ends`;
    return JSON.stringify(trigger);
  }

  function actionLabels(actions: any[]): string[] {
    return actions.map(a => {
      if (typeof a === "string") return a.replace(/_/g, " ");
      const key = Object.keys(a)[0];
      if (!key) return "?";
      const val = a[key];
      if (typeof val === "string") return `${key.replace(/_/g, " ")}: ${val}`;
      if (val?.profile_id) return `Activate profile: ${val.profile_id}`;
      if (val?.preset_id) return `Activate preset: ${val.preset_id}`;
      if (val?.message) return `Notify: ${val.message}`;
      if (val?.format) return `Export: ${val.format}`;
      return key.replace(/_/g, " ");
    });
  }
</script>

<div class="page-header"><h2>Automations</h2></div>
<p class="page-desc">Automations run actions in response to triggers. Enable/disable them as needed.</p>

<div class="automation-grid">
  {#each Object.entries(automations) as [id, a]}
    <div class="automation-card" class:enabled={a.enabled}>
      <div class="automation-card-top">
        <div class="automation-name">{a.name}</div>
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
