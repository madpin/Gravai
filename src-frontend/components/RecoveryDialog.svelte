<script lang="ts">
  import Icon from "./Icon.svelte";
  import { incomplete, recoveryOpen, resolveIncomplete, type IncompleteSession, type RecoveryAction } from "../lib/recovery";
  import { fmtBytes } from "../lib/costs";
  import { fmtDuration } from "../lib/tauri";
  import { addAlert } from "../lib/store";

  // Recordings that did not finish cleanly. One line each; expand for detail.
  // Every action is per recording, plus "Convert all" for the common case.

  let expanded = $state<string | null>(null);
  let busy = $state<Record<string, RecoveryAction>>({});
  let errors = $state<Record<string, string>>({});
  let confirmingDelete = $state<string | null>(null);
  let bulk = $state(false);

  let items = $derived($incomplete);
  let withAudio = $derived(items.filter((s) => s.tracks.length > 0));

  function close() { recoveryOpen.set(false); expanded = null; confirmingDelete = null; }

  function name(s: IncompleteSession): string {
    return s.title || (s.in_library ? "Untitled recording" : "Recording not in your library");
  }

  function when(iso: string | null): string {
    if (!iso) return "Unknown date";
    const d = new Date(iso);
    return d.toLocaleDateString([], { weekday: "short", day: "numeric", month: "short", year: d.getFullYear() === new Date().getFullYear() ? undefined : "numeric" })
      + " · " + d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }

  function stoppedAround(s: IncompleteSession): string {
    if (!s.last_write) return "";
    return new Date(s.last_write).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }

  function length(s: IncompleteSession): string {
    return s.seconds >= 1 ? fmtDuration(s.seconds) : "No audio";
  }

  async function act(s: IncompleteSession, action: RecoveryAction) {
    if (action === "delete" && confirmingDelete !== s.session_id) { confirmingDelete = s.session_id; return; }
    confirmingDelete = null;
    busy = { ...busy, [s.session_id]: action };
    const { [s.session_id]: _, ...rest } = errors; errors = rest;
    try {
      const message = await resolveIncomplete(s.session_id, action);
      if (action !== "delete") addAlert({ level: "info", message: `“${name(s)}”: ${message}.`, dismissable: true });
    } catch (e) {
      errors = { ...errors, [s.session_id]: String(e) };
    } finally {
      const { [s.session_id]: _b, ...left } = busy; busy = left;
    }
    if (!$incomplete.length) close();
  }

  async function convertAll() {
    bulk = true;
    for (const s of [...items]) {
      await act(s, s.tracks.length ? "convert" : "keep");
    }
    bulk = false;
  }

  function verb(action: RecoveryAction): string {
    return action === "convert" ? "Converting…" : action === "keep" ? "Saving…" : "Deleting…";
  }
</script>

<svelte:window onkeydown={(e) => { if ($recoveryOpen && e.key === "Escape" && !bulk) close(); }} />

{#if $recoveryOpen && items.length}
  <div class="scrim" aria-hidden="true"></div>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="recovery-title">
    <header>
      <span class="icon"><Icon name="alert-triangle" size={18} /></span>
      <div>
        <h2 id="recovery-title">{items.length === 1 ? "A recording didn't finish" : `${items.length} recordings didn't finish`}</h2>
        <p>Gravai closed before {items.length === 1 ? "it was" : "they were"} stopped. The audio and transcript up to that point are still here. Choose what to do with each one.</p>
      </div>
    </header>

    <div class="legend">
      <span><strong>Convert</strong> repairs the audio and compresses it into one playable file.</span>
      <span><strong>Keep</strong> repairs it and leaves the raw tracks as they are.</span>
      <span><strong>Delete</strong> removes the audio and the transcript.</span>
    </div>

    <ul class="list">
      {#each items as s (s.session_id)}
        {@const open = expanded === s.session_id}
        <li class:open>
          <div class="row">
            <button class="summary" aria-expanded={open} onclick={() => (expanded = open ? null : s.session_id)}>
              <Icon name={open ? "chevron-down" : "chevron-right"} size={13} />
              <span class="main">
                <span class="name">{name(s)}{#if s.meeting_app}<span class="app">{s.meeting_app}</span>{/if}</span>
                <span class="meta">
                  {when(s.started_at)} · {length(s)}
                  {#if s.raw_bytes} · {fmtBytes(s.raw_bytes)}{/if}
                  {#if s.utterances} · {s.utterances} sentences{/if}
                </span>
              </span>
            </button>
            <div class="actions">
              {#if busy[s.session_id]}
                <span class="working"><Icon name="spinner" size={12} /> {verb(busy[s.session_id])}</span>
              {:else if confirmingDelete === s.session_id}
                <span class="confirm">Delete for good?</span>
                <button class="btn btn-xs btn-ghost" onclick={() => (confirmingDelete = null)}>Cancel</button>
                <button class="btn btn-xs btn-danger-solid" onclick={() => act(s, "delete")}>Delete</button>
              {:else}
                {#if s.tracks.length}
                  <button class="btn btn-xs btn-accent" title="Repair and compress to about {fmtBytes(s.convert_bytes)}" disabled={bulk} onclick={() => act(s, "convert")}>Convert</button>
                {/if}
                <button class="btn btn-xs btn-ghost" title={s.tracks.length ? "Repair and keep the raw tracks" : "Mark it as finished"} disabled={bulk} onclick={() => act(s, "keep")}>{s.tracks.length ? "Keep" : "Mark finished"}</button>
                <button class="btn btn-xs btn-ghost danger" disabled={bulk} onclick={() => act(s, "delete")}>Delete</button>
              {/if}
            </div>
          </div>
          {#if errors[s.session_id]}<p class="err">{errors[s.session_id]}</p>{/if}

          {#if open}
            <div class="detail">
              <ul class="reasons">
                {#each s.reasons as r}<li>{r}</li>{/each}
                {#if stoppedAround(s)}<li>The last audio was written around {stoppedAround(s)}.</li>{/if}
              </ul>

              <dl>
                {#if s.tracks.length}
                  <dt>Audio</dt>
                  <dd>
                    {#each s.tracks as t}
                      <div class="track">
                        <span class="tname">{t.name === "mic" ? "Microphone" : t.name === "system" ? "System audio" : t.name}</span>
                        <span>{fmtDuration(t.seconds)} · {fmtBytes(t.bytes)} · {(t.sample_rate / 1000).toFixed(1)} kHz {t.channels === 1 ? "mono" : "stereo"}</span>
                        {#if !t.header_ok}<span class="tag">Needs repair</span>{/if}
                      </div>
                    {/each}
                    {#if s.segments > 1}<div class="sub">In {s.segments} parts (the recording was continued).</div>{/if}
                    {#if s.tracks.length}<div class="sub">Convert would keep about {fmtBytes(s.convert_bytes)} and free about {fmtBytes(Math.max(0, s.raw_bytes - s.convert_bytes))}.</div>{/if}
                  </dd>
                {:else}
                  <dt>Audio</dt><dd class="sub">No audio files were found.</dd>
                {/if}

                <dt>Transcript</dt>
                <dd>
                  {#if s.preview.length}
                    {#each s.preview as p, i}
                      {#if i === 3 && s.utterances > 4}<div class="gap">… {s.utterances - 4} more …</div>{/if}
                      <div class="line"><span class="who">{p.speaker}</span> {p.text}</div>
                    {/each}
                  {:else}
                    <span class="sub">Nothing was transcribed.</span>
                  {/if}
                </dd>

                {#if s.bookmarks || s.has_summary}
                  <dt>Also saved</dt>
                  <dd>{[s.bookmarks ? `${s.bookmarks} bookmark${s.bookmarks === 1 ? "" : "s"}` : "", s.has_summary ? "a summary" : ""].filter(Boolean).join(" and ")}</dd>
                {/if}
                {#if s.settings}<dt>Recorded with</dt><dd class="sub">{s.settings}</dd>{/if}
                <dt>Folder</dt><dd class="path">{s.dir}</dd>
              </dl>
            </div>
          {/if}
        </li>
      {/each}
    </ul>

    <footer>
      <span class="hint">Decide later and Gravai asks again next time it opens.</span>
      <button class="btn btn-xs btn-ghost" disabled={bulk} onclick={close}>Decide later</button>
      {#if items.length > 1}
        <button class="btn btn-xs btn-accent" disabled={bulk} onclick={convertAll}>
          {bulk ? "Working…" : withAudio.length ? `Convert all ${items.length}` : `Mark all ${items.length} finished`}
        </button>
      {/if}
    </footer>
  </div>
{/if}

<style>
  .scrim { position: fixed; inset: 0; background: rgba(5,5,15,0.6); z-index: 90; }
  .dialog {
    position: fixed; z-index: 91; left: 50%; top: 50%; transform: translate(-50%, -50%);
    width: min(760px, calc(100vw - 48px)); max-height: calc(100vh - 64px);
    display: flex; flex-direction: column; background: var(--bg-primary);
    border: 1px solid var(--border); border-radius: 14px; box-shadow: 0 24px 64px rgba(0,0,0,0.55);
  }
  header { display: flex; gap: 12px; padding: 20px 22px 12px; }
  header .icon { color: var(--warning); padding-top: 2px; }
  h2 { margin: 0 0 4px; font-size: 16px; color: var(--text-primary); }
  header p { margin: 0; font-size: 13px; color: var(--text-secondary); line-height: 1.5; }
  .legend { display: flex; flex-direction: column; gap: 2px; padding: 0 22px 12px 52px; font-size: 12px; color: var(--text-tertiary); }
  .legend strong { color: var(--text-secondary); font-weight: 600; }
  .list { list-style: none; margin: 0; padding: 0 12px; overflow-y: auto; flex: 1; border-top: 1px solid var(--border-subtle); }
  .list > li { border-bottom: 1px solid var(--border-subtle); }
  .row { display: flex; align-items: center; gap: 10px; padding: 8px 6px; }
  .summary {
    flex: 1; min-width: 0; display: flex; align-items: center; gap: 8px; text-align: left;
    background: none; border: none; color: var(--text-tertiary); font: inherit; cursor: pointer; padding: 4px; border-radius: 6px;
  }
  .summary:hover { background: var(--bg-secondary); }
  .main { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .name { font-size: 13px; color: var(--text-primary); display: flex; align-items: center; gap: 6px; }
  .app { font-size: 10px; padding: 1px 6px; border-radius: 4px; background: var(--bg-elevated); color: var(--text-secondary); }
  .meta { font-size: 11px; color: var(--text-tertiary); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .actions { display: flex; align-items: center; gap: 6px; flex-shrink: 0; }
  .danger { color: var(--danger); }
  .btn-danger-solid { background: var(--danger); color: #1a0505; border: none; }
  .confirm { font-size: 12px; color: var(--danger); }
  .working { font-size: 12px; color: var(--text-secondary); display: flex; align-items: center; gap: 6px; }
  .err { margin: 0 6px 8px 34px; font-size: 12px; color: var(--danger); }
  .detail { padding: 2px 10px 14px 34px; display: flex; flex-direction: column; gap: 10px; }
  .reasons { margin: 0; padding-left: 16px; font-size: 12px; color: var(--warning); line-height: 1.5; }
  dl { margin: 0; display: grid; grid-template-columns: 96px 1fr; gap: 8px 12px; font-size: 12px; }
  dt { color: var(--text-tertiary); }
  dd { margin: 0; color: var(--text-secondary); min-width: 0; }
  .track { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .tname { color: var(--text-primary); min-width: 92px; }
  .tag { font-size: 10px; padding: 1px 6px; border-radius: 4px; background: rgba(251,191,36,0.12); color: var(--warning); }
  .sub { color: var(--text-tertiary); }
  .line { line-height: 1.5; }
  .who { color: var(--text-tertiary); margin-right: 4px; }
  .gap { color: var(--text-tertiary); font-style: italic; }
  .path { font-family: "SF Mono", monospace; font-size: 11px; color: var(--text-tertiary); overflow-wrap: anywhere; }
  footer { display: flex; align-items: center; gap: 8px; padding: 12px 22px; border-top: 1px solid var(--border-subtle); }
  .hint { flex: 1; font-size: 11px; color: var(--text-tertiary); }
</style>
