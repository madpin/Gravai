<script lang="ts">
  import { onMount } from "svelte";
  import { config, setValue } from "../../lib/config";
  import { models, loadModels, type ModelEntry } from "../../lib/models";
  import Group from "../../components/settings/Group.svelte";
  import Row from "../../components/settings/Row.svelte";
  import Toggle from "../../components/settings/Toggle.svelte";
  import ModelPicker from "../../components/settings/ModelPicker.svelte";

  let c = $derived($config);

  onMount(loadModels);

  const builtin: ModelEntry = {
    id: "bag-of-words", description: "", approx_size: 0, downloaded: true,
    speed: "Instant, matches words only", ram_gb: undefined,
  };
  let entries = $derived([builtin, ...($models?.embedding_models ?? [])]);
</script>

{#if c}
  <Group title="Search" note="Powers meaning-based search in Archive and the sources Ask uses.">
    <Row label="Search model" hint="A model finds recordings by meaning, not just matching words. After switching, older recordings are re-indexed when you open them." stacked>
      <ModelPicker {entries} active={c.embedding.model} onuse={(id) => setValue("embedding.model", id)} />
    </Row>
    <Row label="Index recordings when opened" hint={c.embedding.auto_index ? "Opening a recording in Archive makes it searchable." : "Only recordings indexed while this was on can be found by meaning."} cost="A few seconds of CPU per recording, once.">
      <Toggle label="Index recordings when opened" checked={c.embedding.auto_index} onchange={(v) => setValue("embedding.auto_index", v)} />
    </Row>
  </Group>
{/if}
