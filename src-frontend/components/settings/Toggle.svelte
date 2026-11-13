<script lang="ts">
  let {
    checked = false,
    disabled = false,
    label = "",
    onchange,
  }: { checked?: boolean; disabled?: boolean; label?: string; onchange?: (value: boolean) => void } = $props();
</script>

<button
  type="button"
  role="switch"
  class="tgl"
  class:on={checked}
  aria-checked={checked}
  aria-label={label}
  {disabled}
  onclick={() => onchange?.(!checked)}
><span class="knob"></span></button>

<style>
  .tgl {
    position: relative; width: 36px; height: 20px; flex-shrink: 0;
    border-radius: 20px; border: 1px solid var(--border); background: var(--bg-base);
    cursor: pointer; padding: 0; transition: background 0.15s, border-color 0.15s;
  }
  .tgl.on { background: var(--accent); border-color: var(--accent); }
  .tgl:disabled { opacity: 0.4; cursor: default; }
  .tgl:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .knob {
    position: absolute; top: 2px; left: 2px; width: 14px; height: 14px; border-radius: 50%;
    background: var(--text-secondary); transition: transform 0.15s, background 0.15s;
  }
  .on .knob { transform: translateX(16px); background: white; }
</style>
