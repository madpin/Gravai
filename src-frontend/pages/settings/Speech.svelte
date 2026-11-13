<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "../../lib/tauri";
  import { config, setValue, LANGUAGES } from "../../lib/config";
  import { models, loadModels, downloadModel, modelName } from "../../lib/models";
  import { modelDownloading, navigate } from "../../lib/store";
  import Group from "../../components/settings/Group.svelte";
  import Row from "../../components/settings/Row.svelte";
  import Toggle from "../../components/settings/Toggle.svelte";
  import More from "../../components/settings/More.svelte";
  import NumberInput from "../../components/settings/NumberInput.svelte";
  import TextInput from "../../components/settings/TextInput.svelte";
  import Segmented from "../../components/settings/Segmented.svelte";
  import ModelPicker from "../../components/settings/ModelPicker.svelte";

  let c = $derived($config);
  let defaultPrompt = $state("");
  let promptDraft = $state("");
  let promptSaved = $state(false);

  onMount(async () => {
    loadModels();
    try { defaultPrompt = (await invoke("get_correction_defaults") as any).default_system_prompt ?? ""; } catch (_) {}
  });

  $effect(() => { promptDraft = c?.correction?.custom_prompt ?? ""; });

  async function savePrompt() {
    await setValue("correction.custom_prompt", promptDraft.trim() || null);
    promptSaved = true;
    setTimeout(() => (promptSaved = false), 1500);
  }

  function aiModel(id: string) {
    return $models?.ai_models?.find(m => m.id === id);
  }

  function lines(list: string[] | undefined): string { return (list ?? []).join("\n"); }
  function toList(text: string): string[] { return text.split("\n").map(s => s.trim()).filter(Boolean); }

  let assistantName = $derived(
    c?.llm?.provider === "api" ? (c?.llm?.model || "the API model") : modelName(c?.llm?.local_model ?? "")
  );
</script>

{#snippet needsModel(id: string, what: string)}
  {@const m = id === "silero_vad" ? $models?.silero_vad : aiModel(id)}
  {#if m && !m.downloaded}
    {#if $modelDownloading[id]}
      <span class="dl">{$modelDownloading[id].progress}%</span>
    {:else}
      <button class="btn btn-xs btn-ghost" title="{what} needs this model" onclick={() => downloadModel(id)}>Download model</button>
    {/if}
  {/if}
{/snippet}

{#if c}
  <Group title="Transcription">
    <Row label="Language" hint="Set it when you know it. Detecting it each time is slower and sometimes wrong." next>
      <select class="select select-sm" value={c.transcription.language} onchange={(e) => setValue("transcription.language", e.currentTarget.value)}>
        {#each LANGUAGES as [v, label]}<option value={v}>{label}</option>{/each}
      </select>
    </Row>
    <Row label="Whisper model" hint="Larger models hear better and need more memory. Everything runs on this Mac." next stacked>
      {#if $models}
        <ModelPicker entries={$models.whisper_models} active={c.transcription.model} onuse={(id) => setValue("transcription.model", id)} />
      {/if}
    </Row>
  </Group>

  <Group title="Analysis" note="Each of these adds work while you record. Turn off what you don't use.">
    <Row label="Speaker labels" hint="Tells voices apart: Speaker 1, Speaker 2." cost="Light extra CPU with the built-in method." next>
      <Toggle label="Speaker labels" checked={c.features.diarization.enabled} onchange={(v) => setValue("features.diarization.enabled", v)} />
    </Row>
    <Row label="Echo suppression" hint="Drops your voice when the mic picks it up from the speakers." cost="Very light." next>
      <Toggle label="Echo suppression" checked={c.features.echo_suppression.enabled} onchange={(v) => setValue("features.echo_suppression.enabled", v)} />
    </Row>
    <Row label="Emotion" hint="Labels the tone of what other people say." cost="Loads a 500 MB model when recording starts and runs it on every remote sentence." next>
      {#if c.features.sentiment.enabled}{@render needsModel("go-emotions", "Emotion")}{/if}
      <Toggle label="Emotion" checked={c.features.sentiment.enabled} onchange={(v) => setValue("features.sentiment.enabled", v)} />
    </Row>
    <Row label="Transcript correction" hint="Fixes names and terms using your Knowledge entries, with {assistantName}." cost="Runs the assistant model during the recording. On a busy Mac this is the slowest part." next>
      <Toggle label="Transcript correction" checked={c.correction.enabled} onchange={(v) => setValue("correction.enabled", v)} />
    </Row>
    {#if c.correction.enabled}
      <Row label="Correct every" hint="Waits for this many sentences, or a pause, before sending a batch.">
        <NumberInput value={c.correction.batch_size} min={3} max={20} unit="sentences" onsave={(v) => setValue("correction.batch_size", v)} />
      </Row>
      <Row label="Or after a pause of">
        <NumberInput value={c.correction.debounce_seconds} min={5} max={60} unit="seconds" onsave={(v) => setValue("correction.debounce_seconds", v)} />
      </Row>
      <Row label="Correction instructions" hint="What the model is told to do. Leave empty for the built-in instructions." stacked>
        <textarea class="config-editor prompt" rows="5" bind:value={promptDraft} placeholder={defaultPrompt}></textarea>
        <div class="prompt-actions">
          <button class="btn btn-xs btn-ghost" onclick={() => navigate("settings:library")}>Knowledge entries →</button>
          <span class="spacer"></span>
          {#if promptSaved}<span class="saved">Saved</span>{/if}
          <button class="btn btn-xs btn-ghost" onclick={() => (promptDraft = "")}>Use built-in</button>
          <button class="btn btn-xs btn-accent" disabled={promptDraft.trim() === (c.correction.custom_prompt ?? "")} onclick={savePrompt}>Save</button>
        </div>
      </Row>
    {/if}

    <More>
      <Row label="Voice detection" hint="Decides where a sentence starts and ends. WebRTC is light. Silero is more accurate in noise and uses more CPU." next>
        {#if c.vad.engine === "silero"}{@render needsModel("silero_vad", "Silero")}{/if}
        <Segmented options={[["webrtc", "WebRTC"], ["silero", "Silero"]]} value={c.vad.engine} onchange={(v) => setValue("vad.engine", v)} />
      </Row>
      {#if c.vad.engine === "silero"}
        <Row label="Silero sensitivity" hint="Lower hears quieter speech and more noise." next>
          <NumberInput value={c.vad.silero.threshold} min={0.1} max={0.9} step={0.05} onsave={(v) => setValue("vad.silero.threshold", v)} />
        </Row>
      {:else}
        <Row label="WebRTC aggressiveness" hint="3 filters the most noise. 0 keeps the most audio." next>
          <NumberInput value={c.vad.webrtc.aggressiveness} min={0} max={3} onsave={(v) => setValue("vad.webrtc.aggressiveness", v)} />
        </Row>
      {/if}
      <Row label="End a sentence after" hint="Silence needed before a sentence is sent to Whisper." next>
        <NumberInput value={c.vad.pause_seconds} min={0.1} max={5} step={0.1} unit="s" onsave={(v) => setValue("vad.pause_seconds", v)} />
      </Row>
      <Row label="Shortest sentence" next>
        <NumberInput value={c.vad.silero.min_utterance_seconds} min={0.1} max={5} step={0.1} unit="s" onsave={(v) => setValue("vad.silero.min_utterance_seconds", v)} />
      </Row>
      <Row label="Longest sentence" hint="Longer chunks keep the CPU busy and delay the transcript." next>
        <NumberInput value={c.vad.silero.max_utterance_seconds} min={5} max={60} unit="s" onsave={(v) => setValue("vad.silero.max_utterance_seconds", v)} />
      </Row>
      <Row label="Speaker method" hint="Built-in needs nothing extra. Pyannote is more accurate and needs a 90 MB model." next>
        {#if c.features.diarization.model === "pyannote"}{@render needsModel("pyannote-segmentation", "Pyannote")}{/if}
        <Segmented options={[["energy", "Built-in"], ["pyannote", "Pyannote"]]} value={c.features.diarization.model} onchange={(v) => setValue("features.diarization.model", v)} />
      </Row>
      <Row label="Most speakers" next>
        <NumberInput value={c.features.diarization.max_speakers} min={2} max={20} onsave={(v) => setValue("features.diarization.max_speakers", v)} />
      </Row>
      <Row label="Echo sensitivity" hint="How alike mic and system text must be to count as echo. Higher drops less." next>
        <NumberInput value={c.features.echo_suppression.similarity_threshold} min={0.3} max={0.95} step={0.05} onsave={(v) => setValue("features.echo_suppression.similarity_threshold", v)} />
      </Row>
      <Row label="Correction time limit" hint="Gives up on a batch that takes longer.">
        <NumberInput value={c.correction.timeout_seconds} min={10} max={600} step={10} unit="s" onsave={(v) => setValue("correction.timeout_seconds", v)} />
      </Row>
      <Row label="Correction model" hint="Leave empty to use the Assistant model.">
        <TextInput value={c.correction.model ?? ""} placeholder={assistantName} width="180px" onsave={(v) => setValue("correction.model", v || null)} />
      </Row>
      <Row label="Ignore these phrases" hint="Whisper sometimes invents them in silence. One per line." stacked next>
        <textarea class="config-editor" rows="4" value={lines(c.transcription.hallucination_blocklist)}
          onblur={(e) => setValue("transcription.hallucination_blocklist", toList(e.currentTarget.value))}></textarea>
      </Row>
      <Row label="Ignore when repeated" hint="Dropped when they repeat back to back. One per line." stacked next>
        <textarea class="config-editor" rows="3" value={lines(c.transcription.hallucination_repeat_blocklist)}
          onblur={(e) => setValue("transcription.hallucination_repeat_blocklist", toList(e.currentTarget.value))}></textarea>
      </Row>
    </More>
  </Group>
{/if}

<style>
  .prompt { min-height: 96px; }
  .prompt-actions { display: flex; align-items: center; gap: 6px; }
  .spacer { flex: 1; }
  .saved { font-size: 11px; color: var(--success); }
  .dl { font-size: 11px; color: var(--text-tertiary); }
  textarea.config-editor { width: 100%; box-sizing: border-box; }
</style>
