<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "../../lib/tauri";
  import { config, setValue, patchConfig } from "../../lib/config";
  import { models, loadModels } from "../../lib/models";
  import { apiHost } from "../../lib/costs";
  import { modelDownloading } from "../../lib/store";
  import Group from "../../components/settings/Group.svelte";
  import Row from "../../components/settings/Row.svelte";
  import More from "../../components/settings/More.svelte";
  import NumberInput from "../../components/settings/NumberInput.svelte";
  import TextInput from "../../components/settings/TextInput.svelte";
  import Segmented from "../../components/settings/Segmented.svelte";
  import ModelPicker from "../../components/settings/ModelPicker.svelte";

  let c = $derived($config);
  let customUrl = $state("");
  let customMsg = $state("");

  onMount(loadModels);

  async function chooseProvider(p: string) {
    if (p === "api") {
      const ok = confirm(
        "With an API provider, transcript text is sent to that service for summaries, chat, and correction.\n\nAudio never leaves this Mac. Continue?"
      );
      if (!ok) return;
    }
    await setValue("llm.provider", p);
  }

  async function downloadCustom() {
    const url = customUrl.trim();
    const filename = url.split("/").pop()?.split("?")[0] ?? "";
    if (!filename.endsWith(".gguf")) { customMsg = "The link must point to a .gguf file."; return; }
    customMsg = "";
    modelDownloading.update(cur => ({ ...cur, [filename]: { progress: 0, status: "starting" } }));
    try {
      await invoke("download_llm_from_url", { url, filename });
      customUrl = "";
      customMsg = `Downloading ${filename}…`;
    } catch (e) {
      modelDownloading.update(cur => { const { [filename]: _, ...rest } = cur; return rest; });
      customMsg = String(e);
    }
  }
</script>

{#if c}
  <Group>
    <Row label="Runs on">
      <Segmented options={[["local", "This Mac"], ["api", "API service"]]} value={c.llm.provider} onchange={chooseProvider} />
    </Row>

    {#if c.llm.provider === "local"}
      <Row label="Model" hint="Nothing leaves this Mac. Larger models write better and need more memory." stacked>
        {#if $models}
          <ModelPicker kind="llm" entries={$models.llm_models} active={c.llm.local_model} onuse={(id) => setValue("llm.local_model", id)} />
        {/if}
      </Row>
      <More label="Add a model from a link">
        <Row label="GGUF download link" hint="Qwen, Llama, Phi, or Mistral in GGUF format. Gemma is not supported." stacked>
          <div class="custom">
            <input class="input" bind:value={customUrl} placeholder="https://huggingface.co/…/model-Q4_K_M.gguf" />
            <button class="btn btn-xs btn-accent" disabled={!customUrl.trim()} onclick={downloadCustom}>Download</button>
          </div>
          {#if customMsg}<span class="msg">{customMsg}</span>{/if}
        </Row>
      </More>
    {:else}
      <div class="notice">
        Audio stays on this Mac. Transcript text is sent to <strong>{apiHost(c.llm.base_url)}</strong> for summaries, Ask, and correction.
      </div>
      <Row label="Service address" hint="Any OpenAI-compatible endpoint.">
        <TextInput value={c.llm.base_url} placeholder="https://api.openai.com/v1" width="280px" onsave={(v) => setValue("llm.base_url", v)} />
      </Row>
      <Row label="Model name">
        <TextInput value={c.llm.model} placeholder="gpt-4o-mini" width="200px" onsave={(v) => setValue("llm.model", v)} />
      </Row>
      <Row label="API key" hint="Stored in Gravai's config file on this Mac. Never copied into saved setups.">
        <TextInput type="password" value={c.llm.api_key ?? ""} placeholder="sk-…" width="200px" onsave={(v) => patchConfig({ llm: { api_key: v || null } })} />
      </Row>
    {/if}

    <More>
      <Row label="Longest answer" hint="Upper limit on what the model writes in one go.">
        <NumberInput value={c.llm.max_tokens} min={256} max={16384} step={256} unit="tokens" onsave={(v) => setValue("llm.max_tokens", v)} />
      </Row>
    </More>
  </Group>
{/if}

<style>
  .custom { display: flex; gap: 8px; }
  .custom .input { flex: 1; max-width: none; }
  .msg { font-size: 12px; color: var(--text-secondary); }
  .notice {
    margin: 12px 18px 4px; padding: 10px 12px; border-radius: 8px; font-size: 12px; line-height: 1.5;
    background: rgba(251,191,36,0.08); border: 1px solid rgba(251,191,36,0.25); color: var(--text-secondary);
  }
  .notice strong { color: var(--text-primary); }
</style>
