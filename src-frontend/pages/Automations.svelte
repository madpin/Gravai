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
    if (typeof trigger === "string") return trigger.replace(/_/g, " ");
    if (trigger?.TimeOfDay) return `Time: ${trigger.TimeOfDay.hour}:${String(trigger.TimeOfDay.minute).padStart(2, "0")}`;
    if (trigger?.AppForegrounded) return `App: ${trigger.AppForegrounded.app_name}`;
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

<div class="card-grid">
  {#each Object.entries(automations) as [id, a]}
    <div class="card" class:active-card={a.enabled}>
      <div class="card-header">
        {a.name}
        <label class="switch">
          <input type="checkbox" checked={a.enabled} onchange={() => toggle(id, !a.enabled)} />
          <span class="switch-slider"></span>
        </label>
      </div>
      <div class="card-body">
        <div><strong>Trigger:</strong> {triggerLabel(a.trigger)}</div>
        <div style="margin-top:4px"><strong>Actions:</strong></div>
        <ul style="margin:4px 0 0 16px">
          {#each actionLabels(a.actions) as label}<li>{label}</li>{/each}
        </ul>
        {#if a.last_run}
          <div style="margin-top:6px; font-size:10px; color:var(--text-tertiary)">
            Last run: {new Date(a.last_run).toLocaleString()} ({a.run_count} times)
          </div>
        {/if}
      </div>
    </div>
  {/each}
</div>
