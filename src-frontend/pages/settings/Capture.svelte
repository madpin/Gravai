<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "../../lib/tauri";
  import { config, setValue, patchConfig } from "../../lib/config";
  import { diskLine } from "../../lib/costs";
  import { isRecording } from "../../lib/store";
  import Group from "../../components/settings/Group.svelte";
  import Row from "../../components/settings/Row.svelte";
  import Toggle from "../../components/settings/Toggle.svelte";
  import More from "../../components/settings/More.svelte";
  import NumberInput from "../../components/settings/NumberInput.svelte";
  import FolderInput from "../../components/settings/FolderInput.svelte";
  import Segmented from "../../components/settings/Segmented.svelte";
  import AppPicker from "../../components/AppPicker.svelte";

  let mics = $state<any[]>([]);
  let apps = $state<any[]>([]);
  let c = $derived($config);
  let rec = $derived(c?.audio?.recording ?? {});

  onMount(async () => {
    try {
      const devices: any[] = await invoke("list_audio_devices");
      const inputs = devices.filter(d => d.device_type === "microphone" || d.device_type === "input");
      mics = inputs.length ? inputs : devices;
    } catch (_) {}
    try { apps = await invoke("list_capturable_apps"); }
    catch (_) { try { apps = await invoke("list_running_apps"); } catch (_) {} }
  });

  async function chooseMic(name: string) {
    await patchConfig({ audio: { microphone: { enabled: true, device_name: name, device_index: -1 } } });
    if ($isRecording) await invoke("switch_microphone", { deviceName: name }).catch(() => {});
  }

  const FORMATS: Array<[string, string, string]> = [
    ["m4a-aac", "M4A (AAC)", "Small files, plays everywhere"],
    ["m4a-alac", "M4A (Apple Lossless)", "Exact audio, about half the size of WAV"],
    ["wav", "WAV", "Exact audio, largest files"],
    ["aiff", "AIFF", "Exact audio, for Apple audio apps"],
    ["caf", "CAF", "Exact audio, Core Audio format"],
  ];
</script>

{#if c}
  <Group title="Sources" note="The same switches as on Record. A change here shows there too.">
    <Row label="Microphone" hint="Your voice and anyone in the room.">
      {#if c.audio.microphone.enabled}
        <select class="select select-sm" value={c.audio.microphone.device_name} onchange={(e) => chooseMic(e.currentTarget.value)}>
          <option value="">System default</option>
          {#if c.audio.microphone.device_name && !mics.some(d => d.name === c.audio.microphone.device_name)}
            <option value={c.audio.microphone.device_name}>{c.audio.microphone.device_name} (disconnected)</option>
          {/if}
          {#each mics as d}<option value={d.name}>{d.name}</option>{/each}
        </select>
      {/if}
      <Toggle label="Microphone" checked={c.audio.microphone.enabled} onchange={(v) => setValue("audio.microphone.enabled", v)} />
    </Row>
    <Row label="System audio" hint="What other people say in a call, or any app's sound." next>
      {#if c.audio.system_audio.enabled}
        <AppPicker {apps} selected={c.audio.system_audio.app_bundle_id} onselect={(v) => setValue("audio.system_audio.app_bundle_id", v)} />
      {/if}
      <Toggle label="System audio" checked={c.audio.system_audio.enabled} onchange={(v) => setValue("audio.system_audio.enabled", v)} />
    </Row>
  </Group>

  <Group title="Audio files">
    <Row label="Save audio" hint={rec.enabled ? "Keep the recording so you can play it back." : "Only the transcript is kept. There is nothing to play back."} next>
      <Toggle label="Save audio" checked={rec.enabled} onchange={(v) => setValue("audio.recording.enabled", v)} />
    </Row>
    {#if rec.enabled}
      <Row label="Format" hint={FORMATS.find(f => f[0] === rec.export_format)?.[2] ?? ""} cost={diskLine(c)} next>
        <select class="select select-sm" value={rec.export_format} onchange={(e) => setValue("audio.recording.export_format", e.currentTarget.value)}>
          {#each FORMATS as [v, label]}<option value={v}>{label}</option>{/each}
        </select>
      </Row>
      <Row label="Compress when a recording stops" hint={rec.auto_compress ? "Raw tracks are replaced by one smaller playback file." : "Raw tracks stay on disk at full size."}>
        <Toggle label="Compress when a recording stops" checked={rec.auto_compress} onchange={(v) => setValue("audio.recording.auto_compress", v)} />
      </Row>
      <More>
        <Row label="Channels" next>
          <Segmented options={[["1", "Mono"], ["2", "Stereo"]]} value={String(rec.channels)} onchange={(v) => setValue("audio.recording.channels", Number(v))} />
        </Row>
        <Row label="Sample rate" hint="48 kHz suits speech and music. Higher only makes files larger." next>
          <select class="select select-sm" value={String(rec.sample_rate)} onchange={(e) => setValue("audio.recording.sample_rate", Number(e.currentTarget.value))}>
            <option value="16000">16 kHz</option><option value="44100">44.1 kHz</option><option value="48000">48 kHz</option><option value="96000">96 kHz</option>
          </select>
        </Row>
        <Row label="Bit depth" hint="24-bit leaves more headroom for editing." next>
          <Segmented options={[["16", "16-bit"], ["24", "24-bit"]]} value={String(rec.bit_depth)} onchange={(v) => setValue("audio.recording.bit_depth", Number(v))} />
        </Row>
        {#if rec.export_format === "m4a-aac"}
          <Row label="AAC bitrate" hint="128 kbps is plenty for speech.">
            <NumberInput value={rec.aac_bitrate_kbps} min={64} max={320} step={32} unit="kbps" onsave={(v) => setValue("audio.recording.aac_bitrate_kbps", v)} />
          </Row>
        {/if}
        <Row label="Recordings folder" hint="Where new recordings are written." next>
          <FolderInput value={rec.output_folder} placeholder="Gravai sessions folder" onsave={(v) => setValue("audio.recording.output_folder", v)} />
        </Row>
      </More>
    {/if}
  </Group>
{/if}
