<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "../lib/tauri";
  import { save } from "@tauri-apps/plugin-dialog";
  import SessionPicker from "../components/SessionPicker.svelte";

  let question = $state("");
  let messages = $state<any[]>([]);
  let loading = $state(false);
  let sessionFilter = $state<string | null>(null);
  let sessions = $state<any[]>([]);
  let conversations = $state<any[]>([]);
  let currentConversationId = $state<string | null>(null);
  let copyFeedback = $state<number | null>(null);

  onMount(async () => {
    try { sessions = await invoke("list_sessions"); } catch (_) {}
    await loadConversations(true);
  });

  async function loadConversations(selectFirst = false) {
    try {
      conversations = await invoke("list_chat_conversations");
      if (selectFirst && conversations.length > 0) {
        await selectConversation(conversations[0].id);
      }
    } catch (_) {}
  }

  async function selectConversation(id: string | null) {
    currentConversationId = id;
    messages = [];
    if (id) {
      try {
        messages = await invoke("get_chat_history", { conversationId: id });
      } catch (_) {}
    }
  }

  async function newChat() {
    try {
      const id: string = await invoke("create_chat_conversation", { sessionId: sessionFilter });
      await loadConversations();
      await selectConversation(id);
    } catch (_) {}
  }

  async function deleteConversation() {
    if (!currentConversationId) return;
    if (!confirm("Delete this conversation?")) return;
    try {
      await invoke("delete_chat_conversation", { conversationId: currentConversationId });
      currentConversationId = null;
      messages = [];
      await loadConversations();
      if (conversations.length > 0) await selectConversation(conversations[0].id);
    } catch (_) {}
  }

  async function exportChat() {
    if (!currentConversationId) return;
    try {
      const path = await save({
        defaultPath: `chat-${currentConversationId.slice(0, 8)}.md`,
        filters: [{ name: "Markdown", extensions: ["md"] }],
      });
      if (path) await invoke("export_chat_markdown_file", { conversationId: currentConversationId, path });
    } catch (_) {}
  }

  async function ask(overrideQuestion?: string) {
    const text = (overrideQuestion ?? question).trim();
    if (!text || loading) return;
    if (!overrideQuestion) question = "";
    messages = [...messages, { role: "user", content: text }];
    loading = true;
    try {
      const response: any = await invoke("ask_gravai", {
        question: text,
        sessionId: sessionFilter,
        conversationId: currentConversationId,
      });
      if (response.conversation_id && !currentConversationId) {
        currentConversationId = response.conversation_id;
        await loadConversations();
      }
      messages = [...messages, { role: "assistant", content: response.answer, citations: response.citations }];
    } catch (e) {
      messages = [...messages, { role: "assistant", content: `Error: ${e}` }];
    }
    loading = false;
  }

  async function retry() {
    const lastUser = [...messages].reverse().find(m => m.role === "user");
    if (lastUser) await ask(lastUser.content);
  }

  async function copyMessage(content: string, idx: number) {
    await navigator.clipboard.writeText(content);
    copyFeedback = idx;
    setTimeout(() => { copyFeedback = null; }, 1500);
  }

  function handleKey(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); ask(); }
  }

  let chatEl: HTMLElement;
  $effect(() => {
    messages;
    if (chatEl) requestAnimationFrame(() => { chatEl.scrollTop = chatEl.scrollHeight; });
  });

  function convLabel(c: any): string {
    const title = c.title ?? `Chat ${c.id.slice(0, 6)}`;
    const d = new Date(c.updated_at);
    const today = new Date();
    const dateStr = d.toDateString() === today.toDateString()
      ? d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })
      : d.toLocaleDateString([], { month: "short", day: "numeric" });
    return `${title} · ${dateStr}`;
  }

  const hasUserMessage = $derived(messages.some(m => m.role === "user"));
</script>

<div class="chat-layout">
  <div class="page-header" style="flex-shrink:0">
    <h2>Ask Gravai</h2>
    <SessionPicker {sessions} selected={sessionFilter} onselect={(id) => { sessionFilter = id; currentConversationId = null; messages = []; }} />
  </div>

  <div class="conversation-bar">
    <select
      class="conv-select"
      value={currentConversationId ?? ""}
      onchange={(e) => selectConversation((e.target as HTMLSelectElement).value || null)}
    >
      <option value="">— select a conversation —</option>
      {#each conversations as c}
        <option value={c.id}>{convLabel(c)}</option>
      {/each}
    </select>
    <button class="btn btn-sm" onclick={newChat} title="Start a new conversation">+ New</button>
    <button
      class="btn btn-sm btn-danger"
      onclick={deleteConversation}
      disabled={!currentConversationId}
      title="Delete this conversation"
    >Delete</button>
    <button
      class="btn btn-sm"
      onclick={exportChat}
      disabled={!currentConversationId || messages.length === 0}
      title="Export as Markdown file"
    >↓ Export</button>
  </div>

  <div class="card chat-container">
    <div class="chat-messages" bind:this={chatEl}>
      {#if messages.length === 0}
        <div class="empty-state">
          Ask anything about your meetings.<br/>
          <span style="font-size:11px">e.g. "What did we decide about the migration?" or "List all action items from this week"</span>
        </div>
      {/if}
      {#each messages as msg, i}
        <div class="chat-msg" class:user={msg.role === "user"} class:assistant={msg.role === "assistant"}>
          <div class="chat-msg-header">
            <div class="chat-role">{msg.role === "user" ? "You" : "🤖 Gravai"}</div>
            <button class="copy-btn" onclick={() => copyMessage(msg.content, i)} title="Copy to clipboard">
              {copyFeedback === i ? "✓" : "📋"}
            </button>
          </div>
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
        <div class="chat-msg assistant">
          <div class="chat-role">🤖 Gravai</div>
          <div class="chat-content thinking">Thinking...</div>
        </div>
      {/if}
    </div>
    <div class="chat-input-row">
      <button
        class="btn btn-sm retry-btn"
        onclick={retry}
        disabled={loading || !hasUserMessage}
        title="Retry last question"
      >↺ Retry</button>
      <textarea
        class="chat-input"
        bind:value={question}
        onkeydown={handleKey}
        placeholder="Ask about your meetings..."
        rows="2"
      ></textarea>
      <button class="btn btn-accent" onclick={() => ask()} disabled={loading || !question.trim()}>Send</button>
    </div>
  </div>
</div>

<style>
  :global(.content:has(.chat-layout)) { overflow: hidden; padding: 16px 20px; gap: 10px; }
  .chat-layout { display: flex; flex-direction: column; flex: 1; min-height: 0; gap: 8px; }

  .conversation-bar { display: flex; gap: 6px; align-items: center; flex-shrink: 0; }
  .conv-select {
    flex: 1; background: var(--bg-elevated); color: var(--text-primary);
    border: 1px solid var(--border); border-radius: 6px;
    padding: 4px 8px; font-size: 12px; cursor: pointer; outline: none;
    max-width: 360px;
  }
  .conv-select:focus { border-color: var(--accent); }
  .btn-sm { padding: 4px 10px; font-size: 12px; }
  .btn-danger { color: var(--danger, #f87171); }
  .btn-danger:not(:disabled):hover { background: var(--danger, #f87171); color: #fff; }

  .chat-container { display: flex; flex-direction: column; flex: 1; min-height: 0; }
  .chat-messages { flex: 1; overflow-y: auto; padding: 16px; display: flex; flex-direction: column; gap: 12px; }

  .chat-msg { max-width: 85%; padding: 10px 14px; border-radius: 12px; }
  .chat-msg.user { align-self: flex-end; background: var(--accent-glow); border: 1px solid var(--accent-dim); }
  .chat-msg.assistant { align-self: flex-start; background: var(--bg-elevated); }

  .chat-msg-header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 4px; }
  .chat-role { font-size: 10px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.5px; color: var(--text-tertiary); }
  .copy-btn {
    background: none; border: none; cursor: pointer; font-size: 11px;
    color: var(--text-tertiary); padding: 0 2px; opacity: 0; transition: opacity 0.15s;
  }
  .chat-msg:hover .copy-btn { opacity: 1; }

  .chat-content { font-size: 13px; line-height: 1.5; white-space: pre-wrap; }
  .chat-content.thinking { color: var(--text-tertiary); font-style: italic; }

  .chat-citations { margin-top: 8px; display: flex; flex-wrap: wrap; gap: 4px; }
  .citation-tag { font-size: 10px; padding: 2px 6px; background: var(--bg-base); border-radius: 3px; color: var(--text-tertiary); cursor: help; }

  .chat-input-row { display: flex; gap: 8px; padding: 12px 16px; border-top: 1px solid var(--border-subtle); align-items: flex-end; }
  .retry-btn { flex-shrink: 0; align-self: flex-end; }
  .chat-input {
    flex: 1; background: var(--bg-base); color: var(--text-primary);
    border: 1px solid var(--border); border-radius: 8px;
    padding: 8px 12px; font-size: 13px; font-family: inherit;
    resize: none; outline: none;
  }
  .chat-input:focus { border-color: var(--accent); }
</style>
