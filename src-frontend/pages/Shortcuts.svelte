<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "../lib/tauri";

  let bindings = $state<Record<string, any>>({});
  let saveStatus = $state<Record<string, string>>({});
  let recordingAction = $state<string | null>(null);

  onMount(load);

  async function load() {
    try {
      const store: any = await invoke("get_shortcuts");
      bindings = store.bindings || {};
    } catch (_) {}
  }

  async function rebind(actionId: string) {
    const b = bindings[actionId];
    if (!b) return;
    saveStatus = { ...saveStatus, [actionId]: "saving..." };
    try {
      await invoke("rebind_shortcut", { actionId, keySequence: b.key_sequence });
      saveStatus = { ...saveStatus, [actionId]: "✓ saved" };
      setTimeout(() => { saveStatus = { ...saveStatus, [actionId]: "" }; }, 1500);
    } catch (e) {
      saveStatus = { ...saveStatus, [actionId]: `✗ ${e}` };
    }
  }

  function toggleGlobal(actionId: string) {
    if (bindings[actionId]) {
      bindings[actionId] = { ...bindings[actionId], is_global: !bindings[actionId].is_global };
    }
  }

  /** Start recording keystrokes for a shortcut */
  function startRecording(actionId: string) {
    recordingAction = actionId;
  }

  /** Handle keydown during recording — capture the key combination */
  function handleKeyCapture(e: KeyboardEvent, actionId: string) {
    if (recordingAction !== actionId) return;
    e.preventDefault();
    e.stopPropagation();

    // Ignore lone modifier keys
    if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;

    const parts: string[] = [];
    if (e.metaKey || e.ctrlKey) parts.push("CmdOrCtrl");
    if (e.shiftKey) parts.push("Shift");
    if (e.altKey) parts.push("Alt");

    // Map special keys
    const keyMap: Record<string, string> = {
      " ": "Space", "ArrowUp": "Up", "ArrowDown": "Down",
      "ArrowLeft": "Left", "ArrowRight": "Right",
      "Escape": "Escape", "Enter": "Enter", "Backspace": "Backspace",
      "Delete": "Delete", "Tab": "Tab",
    };
    const key = keyMap[e.key] || e.key.toUpperCase();
    parts.push(key);

    bindings[actionId] = { ...bindings[actionId], key_sequence: parts.join("+") };
    recordingAction = null;

    // Auto-save after recording
    rebind(actionId);
  }
</script>

<div class="page-header"><h2>Keyboard Shortcuts</h2></div>
<p class="page-desc">Customize keyboard bindings. Click "Record" to capture a new key combination. Global shortcuts work even when Gravai is in the background.</p>

<div class="card">
  <table class="shortcut-table">
    <thead>
      <tr>
        <th>Action</th>
        <th>Description</th>
        <th>Key Binding</th>
        <th>Scope</th>
        <th></th>
      </tr>
    </thead>
    <tbody>
      {#each Object.entries(bindings) as [actionId, b]}
        <tr>
          <td style="font-family: monospace; font-size: 11px; color: var(--text-tertiary)">{actionId}</td>
          <td>{b.description}</td>
          <td>
            <input
              class="key-input"
              class:recording={recordingAction === actionId}
              bind:value={b.key_sequence}
              onkeydown={(e) => handleKeyCapture(e, actionId)}
              placeholder={recordingAction === actionId ? "Press keys..." : ""}
              readonly={recordingAction === actionId}
            />
          </td>
          <td>
            <label class="toggle-label" style="min-width:auto">
              <input type="checkbox" class="toggle" checked={b.is_global} onchange={() => toggleGlobal(actionId)} />
              {b.is_global ? "Global" : "Local"}
            </label>
          </td>
          <td style="display:flex;gap:4px">
            <button class="btn btn-xs" class:btn-accent={recordingAction === actionId} class:btn-ghost={recordingAction !== actionId} onclick={() => startRecording(actionId)}>
              {recordingAction === actionId ? "⏺ Press keys..." : "Record"}
            </button>
            {#if saveStatus[actionId]}
              <span style="font-size:10px;color:var(--text-tertiary)">{saveStatus[actionId]}</span>
            {/if}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

<style>
  .key-input.recording {
    border-color: var(--danger) !important;
    animation: pulse-border 1s infinite;
    color: var(--danger);
  }
  @keyframes pulse-border {
    0%, 100% { box-shadow: 0 0 0 0 rgba(248,113,113,0.3); }
    50% { box-shadow: 0 0 0 3px rgba(248,113,113,0); }
  }
</style>
