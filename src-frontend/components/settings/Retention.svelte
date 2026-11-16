<script lang="ts">
  import { config, setValue } from "../../lib/config";
  import { invoke } from "../../lib/tauri";
  import Group from "./Group.svelte";
  import Row from "./Row.svelte";
  import Toggle from "./Toggle.svelte";
  import NumberInput from "./NumberInput.svelte";
  let c = $derived($config?.retention);
  let busy = $state(false);
  let message = $state("");
  async function run() {
    busy = true;
    try {
      const result: any = await invoke("run_retention_now");
      message = `${result.processed} sessions processed; ${(result.freed_bytes / 1048576).toFixed(1)} MB freed. ${result.errors.join("; ")}`;
    } catch (e) { message = String(e); }
    finally { busy = false; }
  }
</script>
{#if c}
  <Group title="Audio retention" note="For future recordings. Save these settings as a setup to give each setup its own policy. Each session keeps its starting policy, even when you switch setups. Runs at launch and daily; transcripts, summaries and exports stay. Shared custom audio folders are skipped.">
    <Row label="Enable scheduled audio retention" hint="Opting in permits deletion of raw WAVs after the chosen age. This is irreversible; compressed copies lose quality."><Toggle label="Enable retention" checked={c.enabled} onchange={v => setValue("retention.enabled", v)} /></Row>
    {#if c.enabled}
      <Row label="Days after recording ends"><NumberInput value={c.min_age_days} min={1} max={36500} onsave={v => setValue("retention.min_age_days", v)} /></Row>
      <Row label="Action"><select aria-label="Retention action" value={c.raw_wav_action} onchange={e => setValue("retention.raw_wav_action", e.currentTarget.value)}><option value="transcode_compressed">Keep compressed AAC, remove WAVs</option><option value="delete">Remove WAVs, keep existing compressed copies</option></select></Row>
      {#if c.raw_wav_action === "transcode_compressed"}<Row label="AAC bitrate (kbps)" hint="96 kbps total by default for mono or stereo. Higher values keep more detail."><NumberInput value={c.bitrate_kbps} min={32} max={320} onsave={v => setValue("retention.bitrate_kbps", v)} /></Row>{/if}
    {/if}
    <Row label="Apply saved session policies" hint="Stop recording first. Current settings never enroll older sessions."><button class="btn btn-xs btn-ghost" disabled={busy} onclick={run}>{busy ? "Running…" : "Run retention now"}</button></Row>
    {#if message}<p style="padding: 8px 18px; font-size: 12px" role="status">{message}</p>{/if}
  </Group>
{/if}
