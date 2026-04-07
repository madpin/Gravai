<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "../lib/tauri";
  import SessionPicker from "../components/SessionPicker.svelte";

  let question = $state("");
  let messages = $state<any[]>([]);
  let loading = $state(false);
  let sessionFilter = $state<string | null>(null);
  let sessions = $state<any[]>([]);

  onMount(async () => {
    try { sessions = await invoke("list_sessions"); } catch (_) {}
    await loadHistory();
  });

  async function loadHistory() {
    try {
      messages = await invoke("get_chat_history", { sessionId: sessionFilter });
    } catch (_) {}
  }

  async function ask() {
    if (!question.trim() || loading) return;
    const q = question.trim();
    question = "";
    messages = [...messages, { role: "user", content: q }];
    loading = true;

    try {
      const response: any = await invoke("ask_gravai", {
        question: q,
        sessionId: sessionFilter,
      });
      messages = [
        ...messages,
        {
          role: "assistant",
          content: response.answer,
          citations: response.citations,
        },
      ];
    } catch (e) {
      messages = [...messages, { role: "assistant", content: `Error: ${e}` }];
    }
    loading = false;
  }

  function handleKey(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      ask();
    }
  }

  let chatEl: HTMLElement;
  $effect(() => {
    messages;
    if (chatEl) requestAnimationFrame(() => { chatEl.scrollTop = chatEl.scrollHeight; });
  });
</script>

<div class="chat-layout">
<div class="page-header" style="flex-shrink:0">
  <h2>Ask Gravai</h2>
  <SessionPicker {sessions} selected={sessionFilter} onselect={(id) => { sessionFilter = id; loadHistory(); }} />
</div>

<div class="card chat-container">
  <div class="chat-messages" bind:this={chatEl}>
    {#if messages.length === 0}
      <div class="empty-state">
        Ask anything about your meetings.<br/>
        <span style="font-size:11px">e.g. "What did we decide about the migration?" or "List all action items from this week"</span>
      </div>
    {/if}
    {#each messages as msg}
      <div class="chat-msg" class:user={msg.role === "user"} class:assistant={msg.role === "assistant"}>
        <div class="chat-role">{msg.role === "user" ? "You" : "🤖 Gravai"}</div>
        <div class="chat-content">{msg.content}</div>
        {#if msg.citations?.length}
          <div class="chat-citations">
            {#each msg.citations as c}
              <span class="citation-tag" title="{c.text_snippet}">
                📎 {c.session_id.slice(0, 8)} [{c.timestamp}]
              </span>
            {/each}
          </div>
        {/if}
      </div>
    {/each}
    {#if loading}
      <div class="chat-msg assistant"><div class="chat-role">🤖 Gravai</div><div class="chat-content thinking">Thinking...</div></div>
    {/if}
  </div>
  <div class="chat-input-row">
    <textarea class="chat-input" bind:value={question} onkeydown={handleKey} placeholder="Ask about your meetings..." rows="2"></textarea>
    <button class="btn btn-accent" onclick={ask} disabled={loading || !question.trim()}>Send</button>
  </div>
</div>
</div>

<style>
  :global(.content:has(.chat-layout)) { overflow: hidden; padding: 16px 20px; gap: 10px; }
  .chat-layout { display: flex; flex-direction: column; flex: 1; min-height: 0; }
  .chat-container { display: flex; flex-direction: column; flex: 1; min-height: 0; }
  .chat-messages { flex: 1; overflow-y: auto; padding: 16px; display: flex; flex-direction: column; gap: 12px; }
  .chat-msg { max-width: 85%; padding: 10px 14px; border-radius: 12px; }
  .chat-msg.user { align-self: flex-end; background: var(--accent-glow); border: 1px solid var(--accent-dim); }
  .chat-msg.assistant { align-self: flex-start; background: var(--bg-elevated); }
  .chat-role { font-size: 10px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.5px; color: var(--text-tertiary); margin-bottom: 4px; }
  .chat-content { font-size: 13px; line-height: 1.5; white-space: pre-wrap; }
  .chat-content.thinking { color: var(--text-tertiary); font-style: italic; }
  .chat-citations { margin-top: 8px; display: flex; flex-wrap: wrap; gap: 4px; }
  .citation-tag { font-size: 10px; padding: 2px 6px; background: var(--bg-base); border-radius: 3px; color: var(--text-tertiary); cursor: help; }
  .chat-input-row { display: flex; gap: 8px; padding: 12px 16px; border-top: 1px solid var(--border-subtle); align-items: flex-end; }
  .chat-input {
    flex: 1; background: var(--bg-base); color: var(--text-primary);
    border: 1px solid var(--border); border-radius: 8px;
    padding: 8px 12px; font-size: 13px; font-family: inherit;
    resize: none; outline: none;
  }
  .chat-input:focus { border-color: var(--accent); }
</style>
