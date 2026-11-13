<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "../../lib/tauri";
  import { config, setValue } from "../../lib/config";
  import Group from "../../components/settings/Group.svelte";
  import Row from "../../components/settings/Row.svelte";
  import Toggle from "../../components/settings/Toggle.svelte";
  import More from "../../components/settings/More.svelte";
  import NumberInput from "../../components/settings/NumberInput.svelte";
  import FolderInput from "../../components/settings/FolderInput.svelte";

  let c = $derived($config);
  let automations = $state<Record<string, any>>({});
  let setupNames = $state<Record<string, string>>({});
  let calAuth = $state("");
  let calBusy = $state(false);
  let calMsg = $state("");

  // The transcript export rule is the same thing as the export toggle below,
  // so it is not listed twice.
  const EXPORT_RULE = "auto-export-on-stop";
  const ORDER = ["auto-meeting-record", "zoom-auto-stop", "silence-auto-stop"];

  onMount(async () => {
    await loadAutomations();
    try {
      const setups: any[] = await invoke("list_setups");
      setupNames = Object.fromEntries(setups.map(s => [s.id, s.name]));
    } catch (_) {}
    await loadCalendar();
  });

  async function loadAutomations() {
    try { automations = ((await invoke("get_automations")) as any).automations ?? {}; } catch (_) {}
  }

  async function toggleRule(id: string, enabled: boolean) {
    await invoke("toggle_automation", { automationId: id, enabled });
    automations[id] = { ...automations[id], enabled };
  }

  let rules = $derived(
    Object.entries(automations)
      .filter(([id]) => id !== EXPORT_RULE)
      .sort((a, b) => {
        const ia = ORDER.indexOf(a[0]), ib = ORDER.indexOf(b[0]);
        return (ia === -1 ? 99 : ia) - (ib === -1 ? 99 : ib);
      }),
  );

  let exportOn = $derived(!!c?.export?.auto_export_transcript || !!automations[EXPORT_RULE]?.enabled);

  async function setExport(on: boolean) {
    await setValue("export.auto_export_transcript", on);
    if (!on && automations[EXPORT_RULE]?.enabled) await toggleRule(EXPORT_RULE, false);
  }

  function actionText(actions: any[]): string {
    return actions.map(a => {
      if (a === "start_recording") return "start recording";
      if (a === "stop_recording") return "stop recording";
      if (a?.apply_setup) return `apply “${setupNames[a.apply_setup.setup_id] ?? a.apply_setup.setup_id}”`;
      if (a?.show_notification) return "notify";
      if (a?.run_export) return `save a ${a.run_export.format} transcript`;
      return typeof a === "string" ? a.replace(/_/g, " ") : Object.keys(a)[0]?.replace(/_/g, " ");
    }).join(", then ");
  }

  async function loadCalendar() {
    try { calAuth = await invoke("get_calendar_authorization"); } catch (_) { calAuth = ""; }
  }

  async function fixCalendar() {
    calBusy = true; calMsg = "";
    try {
      if (calAuth === "not_determined") calAuth = await invoke("request_calendar_access");
      if (calAuth !== "full_access") {
        await invoke("open_privacy_settings", { permission: "calendar" });
        calMsg = "Turn on Gravai under Calendars, then come back.";
      }
    } catch (e) { calMsg = String(e); }
    calBusy = false;
  }

  const CAL: Record<string, { text: string; ok: boolean }> = {
    full_access: { text: "Calendar access granted.", ok: true },
    write_only: { text: "Gravai can add events but not read them. Reading is needed for names.", ok: false },
    not_determined: { text: "Gravai has not asked for calendar access yet.", ok: false },
    denied: { text: "Calendar access is off in System Settings.", ok: false },
    restricted: { text: "Calendar access is blocked by a device policy.", ok: false },
  };

  function appsText(list: string[]): string { return (list ?? []).join(", "); }
  function toApps(text: string): string[] { return text.split(",").map(s => s.trim()).filter(Boolean); }
</script>

{#if c}
  <Group title="Meetings">
    <Row label="Detect meetings" hint="Notices when Zoom, Teams, Meet, Slack, FaceTime, Discord, or Webex joins a call." cost="Checks running apps every few seconds in the background.">
      <Toggle label="Detect meetings" checked={c.features.meeting_detection.enabled} onchange={(v) => setValue("features.meeting_detection.enabled", v)} />
    </Row>
    <Row label="Name recordings from the calendar" hint="Uses the title of the event happening now." cost="Can take up to a minute on large calendars. Recording starts right away either way." next>
      <Toggle label="Name recordings from the calendar" checked={c.features.meeting_detection.calendar_naming} onchange={(v) => setValue("features.meeting_detection.calendar_naming", v)} />
    </Row>
    {#if c.features.meeting_detection.calendar_naming && calAuth && calAuth !== "unsupported"}
      <div class="cal" class:ok={CAL[calAuth]?.ok}>
        <span>{CAL[calAuth]?.text ?? calAuth}{#if calMsg} {calMsg}{/if}</span>
        {#if !CAL[calAuth]?.ok}
          <button class="btn btn-xs btn-ghost" disabled={calBusy} onclick={fixCalendar}>{calAuth === "not_determined" ? "Allow…" : "Open System Settings"}</button>
          <button class="btn btn-xs btn-ghost" onclick={loadCalendar}>Check again</button>
        {/if}
      </div>
    {/if}
    {#if c.features.meeting_detection.enabled}
      <More>
        <Row label="Only these apps" hint="Comma separated. Empty means every supported app.">
          <input class="input" style="width:240px" value={appsText(c.features.meeting_detection.allowed_apps)} placeholder="All supported apps"
            onblur={(e) => setValue("features.meeting_detection.allowed_apps", toApps(e.currentTarget.value))} />
        </Row>
        <Row label="Start early by" hint="For calendar-driven starts.">
          <NumberInput value={c.features.meeting_detection.lead_time_seconds} min={0} max={600} step={15} unit="s" onsave={(v) => setValue("features.meeting_detection.lead_time_seconds", v)} />
        </Row>
        <Row label="Meeting counts as ended after" hint="How long the call window must be gone.">
          <NumberInput value={c.features.meeting_detection.idle_timeout_seconds} min={5} max={600} step={5} unit="s" onsave={(v) => setValue("features.meeting_detection.idle_timeout_seconds", v)} />
        </Row>
      </More>
    {/if}
  </Group>

  <Group title="Automatic start and stop" note="Each rule uses your current settings. None of them change your settings.">
    {#each rules as [id, a] (id)}
      <Row label={a.name} hint={a.description} cost={`When ${a.trigger === "meeting_detected" ? "a meeting starts" : a.trigger === "meeting_ended" ? "the meeting ends" : a.trigger === "audio_silent" ? "audio stays quiet" : String(a.trigger).replace(/_/g, " ")}: ${actionText(a.actions)}.${a.last_run ? ` Last ran ${new Date(a.last_run).toLocaleDateString()}.` : ""}`}>
        <Toggle label={a.name} checked={a.enabled} onchange={(v) => toggleRule(id, v)} />
      </Row>
    {/each}
  </Group>

  <Group title="After a recording">
    <Row label="Save the transcript" hint="Writes a Markdown file to the transcript folder when a recording stops.">
      <Toggle label="Save the transcript" checked={exportOn} onchange={setExport} />
    </Row>
    <Row label="Transcript folder">
      <FolderInput value={c.export.transcript_folder} placeholder="Gravai exports folder" onsave={(v) => setValue("export.transcript_folder", v)} />
    </Row>
    <Row label="Save while recording" hint="Writes the transcript every 30 seconds, so a crash loses almost nothing." next>
      <Toggle label="Save while recording" checked={c.export.realtime_save} onchange={(v) => setValue("export.realtime_save", v)} />
    </Row>
  </Group>
{/if}

<style>
  .cal {
    display: flex; align-items: center; gap: 8px; flex-wrap: wrap;
    padding: 8px 18px 12px; font-size: 12px; color: var(--warning); border-bottom: 1px solid var(--border-subtle);
  }
  .cal.ok { color: var(--text-tertiary); }
  .cal span { flex: 1; min-width: 200px; }
</style>
