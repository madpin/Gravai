<script lang="ts">
  // Saves on blur or Enter, not on every keystroke.
  let {
    value = "",
    placeholder = "",
    type = "text",
    width = "240px",
    onsave,
  }: { value?: string; placeholder?: string; type?: string; width?: string; onsave: (value: string) => void } = $props();
  let draft = $state("");
  $effect(() => { draft = value ?? ""; });
  function commit() { if (draft !== (value ?? "")) onsave(draft.trim()); }
</script>

<input
  class="input"
  style="width:{width}"
  {type}
  {placeholder}
  bind:value={draft}
  onblur={commit}
  onkeydown={(e) => { if (e.key === "Enter") (e.currentTarget as HTMLInputElement).blur(); }}
/>
