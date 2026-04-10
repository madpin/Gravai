<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "../lib/tauri";
  import { save } from "@tauri-apps/plugin-dialog";
  import Icon from "../components/Icon.svelte";
  import { currentPage, pendingArchiveSessionId } from "../lib/store";

  let question = $state("");
  let messages = $state<any[]>([]);
  let loading = $state(false);
  let sessionFilters = $state<string[]>([]);  // multi-select session context
  let sessions = $state<any[]>([]);
  let conversations = $state<any[]>([]);
  let currentConversationId = $state<string | null>(null);
  let copyFeedback = $state<number | null>(null);

  // Context picker dropdown state
  let contextOpen = $state(false);
  let contextQuery = $state("");
  let contextAnchor: HTMLElement;

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
      const id: string = await invoke("create_chat_conversation", {
        sessionId: sessionFilters[0] ?? null,
      });
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
        sessionId: sessionFilters[0] ?? null,
        sessionIds: sessionFilters.length > 0 ? sessionFilters : null,
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

  function openCitation(sessionId: string) {
    pendingArchiveSessionId.set(sessionId);
    currentPage.set("archive");
  }

  function handleKey(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); ask(); }
  }

  function toggleSession(id: string) {
    if (sessionFilters.includes(id)) {
      sessionFilters = sessionFilters.filter(x => x !== id);
    } else {
      sessionFilters = [...sessionFilters, id];
    }
  }

  function removeSession(id: string) {
    sessionFilters = sessionFilters.filter(x => x !== id);
  }

  function sessionTitle(id: string): string {
    return sessions.find(s => s.id === id)?.title || id.slice(0, 8);
  }

  function handleContextBlur() {
    setTimeout(() => { contextOpen = false; contextQuery = ""; }, 200);
  }

  const filteredSessions = $derived(
    sessions.filter(s => {
      if (!contextQuery.trim()) return true;
      const q = contextQuery.toLowerCase();
      return (s.title || "").toLowerCase().includes(q) || (s.meeting_app || "").toLowerCase().includes(q);
    })
  );

  let chatEl: HTMLElement;
  $effect(() => {
    messages;
    if (chatEl) requestAnimationFrame(() => { chatEl.scrollTop = chatEl.scrollHeight; });
  });

  function convLabel(c: any): string {
    return c.title ?? `Chat ${c.id.slice(0, 6)}`;
  }

  function convDate(c: any): string {
    const d = new Date(c.updated_at);
    const today = new Date();
    if (d.toDateString() === today.toDateString()) {
      return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    }
    const yesterday = new Date(today);
    yesterday.setDate(today.getDate() - 1);
    if (d.toDateString() === yesterday.toDateString()) return "Yesterday";
    return d.toLocaleDateString([], { month: "short", day: "numeric" });
  }

  const hasUserMessage = $derived(messages.some(m => m.role === "user"));
  const currentConv = $derived(conversations.find(c => c.id === currentConversationId));
</script>

<div class="chat-layout">
  <!-- Left sidebar: conversation list -->
  <div class="chat-sidebar">
    <div class="sidebar-header">
      <span class="sidebar-title">Conversations</span>
      <button class="btn-icon" onclick={newChat} title="New conversation">
        <Icon name="plus" size={14}/>
      </button>
    </div>
    <div class="conv-list">
      {#if conversations.length === 0}
        <div class="conv-empty">No conversations yet.<br/>Start one by asking a question.</div>
      {/if}
      {#each conversations as c}
        <button
          class="conv-item"
          class:active={c.id === currentConversationId}
          onclick={() => selectConversation(c.id)}
        >
          <div class="conv-item-title">{convLabel(c)}</div>
          <div class="conv-item-meta">
            <span>{convDate(c)}</span>
            {#if c.message_count > 0}
              <span class="conv-item-count">{c.message_count} msg{c.message_count === 1 ? "" : "s"}</span>
            {/if}
          </div>
        </button>
      {/each}
    </div>
  </div>

  <!-- Right panel: messages + input -->
  <div class="card chat-panel">
    <!-- Panel header: title + context picker + actions -->
    <div class="panel-header">
      <div class="panel-title">
        {#if currentConv}
          <span>{convLabel(currentConv)}</span>
        {:else}
          <span class="panel-title-empty">Ask Gravai</span>
        {/if}
      </div>

      <!-- Context multi-session picker -->
      <div class="context-picker" bind:this={contextAnchor}>
        <div class="context-label">Context</div>
        <div class="context-chips" class:open={contextOpen}>
          {#if sessionFilters.length === 0}
            <button class="context-all" onclick={() => { contextOpen = !contextOpen; }}>
              All meetings <Icon name="chevron-down" size={11}/>
            </button>
          {:else}
            {#each sessionFilters as id}
              <span class="context-chip">
                {sessionTitle(id)}
                <button class="chip-remove" onclick={() => removeSession(id)} title="Remove">&times;</button>
              </span>
            {/each}
            <button class="context-add" onclick={() => { contextOpen = !contextOpen; }} title="Add session">
              <Icon name="plus" size={12}/>
            </button>
          {/if}
        </div>

        {#if contextOpen}
          <div class="context-dropdown" role="listbox" onmouseleave={handleContextBlur}>
            <!-- svelte-ignore a11y_autofocus -->
            <input
              class="context-search"
              type="text"
              placeholder="Search meetings…"
              bind:value={contextQuery}
              autofocus
            />
            <div class="context-options">
              {#each filteredSessions as s}
                <button
                  class="context-option"
                  class:selected={sessionFilters.includes(s.id)}
                  onclick={() => toggleSession(s.id)}
                >
                  <span class="context-option-check">
                    {#if sessionFilters.includes(s.id)}<Icon name="check" size={12}/>{/if}
                  </span>
                  <span class="context-option-text">
                    <span class="context-option-title">{s.title || s.id.slice(0, 12)}</span>
                    {#if s.meeting_app}<span class="context-option-app">{s.meeting_app}</span>{/if}
                  </span>
                </button>
              {/each}
              {#if filteredSessions.length === 0}
                <div class="context-empty">No meetings found</div>
              {/if}
            </div>
          </div>
        {/if}
      </div>

      <div class="panel-actions">
        <button class="btn-icon" onclick={exportChat} disabled={!currentConversationId || messages.length === 0} title="Export as Markdown">
          <Icon name="save" size={14}/>
        </button>
        <button class="btn-icon btn-icon-danger" onclick={deleteConversation} disabled={!currentConversationId} title="Delete conversation">
          <Icon name="trash" size={14}/>
        </button>
      </div>
    </div>

    <div class="chat-messages" bind:this={chatEl}>
      {#if messages.length === 0}
        <div class="empty-state">
          Ask anything about your meetings.<br/>
          <span style="font-size:11px;opacity:0.7">e.g. "What did we decide about the migration?" or "List all action items from this week"</span>
        </div>
      {/if}
      {#each messages as msg, i}
        <div class="chat-msg" class:user={msg.role === "user"} class:assistant={msg.role === "assistant"}>
          <div class="chat-msg-header">
            <div class="chat-role">
              {#if msg.role === "user"}<Icon name="user" size={13}/> You{:else}<Icon name="bot" size={13}/> Gravai{/if}
            </div>
            <button class="copy-btn" onclick={() => copyMessage(msg.content, i)} title="Copy to clipboard">
              <Icon name={copyFeedback === i ? "check" : "clipboard"} size={13}/>
            </button>
          </div>
          <div class="chat-content">{msg.content}</div>
          {#if msg.citations?.length}
            <div class="chat-citations">
              {#each msg.citations as c}
                <button class="citation-tag" title="Go to session in Archive" onclick={() => openCitation(c.session_id)}>
                  <Icon name="paperclip" size={11}/> {c.session_id.slice(0, 8)} [{c.timestamp}]
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {/each}
      {#if loading}
        <div class="chat-msg assistant">
          <div class="chat-role"><Icon name="bot" size={13}/> Gravai</div>
          <div class="chat-content thinking">Thinking...</div>
        </div>
      {/if}
    </div>
    <div class="chat-input-row">
      <textarea
        class="chat-input"
        bind:value={question}
        onkeydown={handleKey}
        placeholder="Ask about your meetings…"
        rows="2"
      ></textarea>
      <div class="chat-input-actions">
        <button
          class="btn-icon retry-btn"
          onclick={retry}
          disabled={loading || !hasUserMessage}
          title="Retry last question"
        ><Icon name="refresh" size={14}/></button>
        <button class="btn btn-accent send-btn" onclick={() => ask()} disabled={loading || !question.trim()}>
          <Icon name="send" size={14}/> Send
        </button>
      </div>
    </div>
  </div>
</div>

<style>
  :global(.content:has(.chat-layout)) { overflow: hidden; padding: 16px 20px; gap: 10px; }
  .chat-layout { display: flex; flex-direction: row; flex: 1; min-height: 0; gap: 12px; }

  /* Sidebar */
  .chat-sidebar {
    display: flex; flex-direction: column;
    width: 210px; flex-shrink: 0;
    background: var(--bg-elevated); border-radius: 10px;
    border: 1px solid var(--border); min-height: 0; overflow: hidden;
  }
  .sidebar-header {
    display: flex; align-items: center; justify-content: space-between;
    padding: 10px 12px; border-bottom: 1px solid var(--border-subtle);
    flex-shrink: 0;
  }
  .sidebar-title { font-size: 11px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.5px; color: var(--text-tertiary); }
  .conv-list { flex: 1; overflow-y: auto; padding: 4px; }
  .conv-empty { font-size: 11px; color: var(--text-tertiary); padding: 16px 10px; text-align: center; line-height: 1.6; }
  .conv-item {
    display: flex; flex-direction: column; align-items: flex-start;
    width: 100%; padding: 8px 10px; border-radius: 6px;
    background: none; border: none; cursor: pointer; text-align: left;
    transition: background 0.12s;
  }
  .conv-item:hover { background: var(--bg-hover); }
  .conv-item.active { background: var(--accent-glow); }
  .conv-item-title {
    font-size: 12px; font-weight: 500; color: var(--text-primary);
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis; width: 100%;
  }
  .conv-item-meta { display: flex; align-items: center; justify-content: space-between; margin-top: 2px; width: 100%; }
  .conv-item-meta > span:first-child { font-size: 10px; color: var(--text-tertiary); }
  .conv-item-count { font-size: 10px; color: var(--text-tertiary); background: var(--bg-base); border-radius: 3px; padding: 0 4px; }

  /* Right panel */
  .chat-panel { display: flex; flex-direction: column; flex: 1; min-height: 0; min-width: 0; }

  .panel-header {
    display: flex; align-items: center; gap: 10px;
    padding: 8px 12px; border-bottom: 1px solid var(--border-subtle); flex-shrink: 0;
  }
  .panel-title {
    font-size: 13px; font-weight: 600; color: var(--text-primary);
    min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    flex-shrink: 1; flex-basis: 160px;
  }
  .panel-title-empty { color: var(--text-tertiary); font-weight: 400; }
  .panel-actions { display: flex; gap: 2px; flex-shrink: 0; margin-left: auto; }

  /* Context multi-picker */
  .context-picker {
    display: flex; align-items: center; gap: 6px;
    flex: 1; min-width: 0; position: relative;
  }
  .context-label {
    font-size: 10px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.4px;
    color: var(--text-tertiary); flex-shrink: 0;
  }
  .context-chips {
    display: flex; align-items: center; flex-wrap: wrap; gap: 4px;
    flex: 1; min-width: 0;
  }
  .context-all {
    display: inline-flex; align-items: center; gap: 4px;
    font-size: 11px; color: var(--text-tertiary);
    background: var(--bg-base); border: 1px solid var(--border);
    border-radius: 5px; padding: 3px 8px; cursor: pointer;
    transition: background 0.12s, border-color 0.12s, color 0.12s;
  }
  .context-all:hover, .context-chips.open .context-all {
    background: var(--bg-elevated); border-color: var(--accent-dim); color: var(--text-primary);
  }
  .context-chip {
    display: inline-flex; align-items: center; gap: 3px;
    font-size: 11px; color: var(--accent);
    background: var(--accent-glow); border: 1px solid var(--accent-dim);
    border-radius: 5px; padding: 2px 6px;
    max-width: 140px;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .chip-remove {
    background: none; border: none; color: inherit; cursor: pointer;
    font-size: 13px; line-height: 1; padding: 0 0 0 2px; opacity: 0.7; flex-shrink: 0;
  }
  .chip-remove:hover { opacity: 1; }
  .context-add {
    display: inline-flex; align-items: center; justify-content: center;
    width: 22px; height: 22px; border-radius: 5px;
    background: var(--bg-base); border: 1px dashed var(--border);
    color: var(--text-tertiary); cursor: pointer;
    transition: background 0.12s, border-color 0.12s, color 0.12s;
  }
  .context-add:hover { background: var(--bg-elevated); border-color: var(--accent-dim); color: var(--accent); }

  /* Context dropdown */
  .context-dropdown {
    position: absolute; top: calc(100% + 6px); left: 0;
    min-width: 260px; max-width: 320px;
    background: var(--bg-primary); border: 1px solid var(--border);
    border-radius: 8px; z-index: 100;
    box-shadow: 0 8px 24px rgba(0,0,0,0.4);
    overflow: hidden;
  }
  .context-search {
    width: 100%; padding: 8px 12px; font-size: 12px; font-family: inherit;
    background: transparent; border: none; border-bottom: 1px solid var(--border-subtle);
    color: var(--text-primary); outline: none;
    box-sizing: border-box;
  }
  .context-search::placeholder { color: var(--text-tertiary); }
  .context-options { max-height: 240px; overflow-y: auto; }
  .context-option {
    display: flex; align-items: flex-start; gap: 8px;
    width: 100%; padding: 7px 12px;
    background: none; border: none; cursor: pointer; text-align: left;
    transition: background 0.1s;
  }
  .context-option:hover { background: var(--bg-elevated); }
  .context-option.selected { background: rgba(124,108,255,0.08); }
  .context-option-check { width: 16px; flex-shrink: 0; color: var(--accent); margin-top: 1px; }
  .context-option-text { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .context-option-title { font-size: 12px; font-weight: 500; color: var(--text-primary); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .context-option-app { font-size: 10px; color: var(--text-tertiary); }
  .context-empty { font-size: 11px; color: var(--text-tertiary); padding: 12px; text-align: center; font-style: italic; }

  /* Icon-only buttons */
  .btn-icon {
    display: flex; align-items: center; justify-content: center;
    width: 28px; height: 28px; border-radius: 6px;
    background: none; border: 1px solid transparent;
    color: var(--text-secondary); cursor: pointer;
    transition: background 0.15s, color 0.15s, border-color 0.15s;
  }
  .btn-icon:hover { background: var(--bg-elevated); border-color: var(--border); color: var(--text-primary); }
  .btn-icon:disabled { opacity: 0.35; cursor: default; pointer-events: none; }
  .btn-icon-danger:hover { color: var(--danger); border-color: rgba(248,113,113,0.3); background: rgba(248,113,113,0.08); }

  /* Chat area */
  .chat-messages { flex: 1; overflow-y: auto; padding: 16px; display: flex; flex-direction: column; gap: 12px; }

  .chat-msg { max-width: 82%; padding: 10px 14px; border-radius: 12px; }
  .chat-msg.user { align-self: flex-end; background: var(--accent-glow); border: 1px solid var(--accent-dim); }
  .chat-msg.assistant { align-self: flex-start; background: var(--bg-elevated); }

  .chat-msg-header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 4px; }
  .chat-role { font-size: 10px; font-weight: 700; text-transform: uppercase; letter-spacing: 0.5px; color: var(--text-tertiary); display: flex; align-items: center; gap: 4px; }
  .copy-btn {
    display: flex; align-items: center;
    background: none; border: none; cursor: pointer;
    color: var(--text-tertiary); padding: 2px 4px; border-radius: 4px;
    opacity: 0; transition: opacity 0.15s, background 0.15s;
  }
  .copy-btn:hover { background: var(--bg-hover); opacity: 1 !important; }
  .chat-msg:hover .copy-btn { opacity: 0.6; }

  .chat-content { font-size: 13px; line-height: 1.5; white-space: pre-wrap; }
  .chat-content.thinking { color: var(--text-tertiary); font-style: italic; }

  .chat-citations { margin-top: 8px; display: flex; flex-wrap: wrap; gap: 4px; }
  .citation-tag {
    display: inline-flex; align-items: center; gap: 3px;
    font-size: 10px; padding: 2px 7px;
    background: var(--bg-base); border-radius: 3px;
    color: var(--text-tertiary);
    border: 1px solid var(--border-subtle);
    cursor: pointer;
    transition: background 0.12s, color 0.12s, border-color 0.12s;
    font-family: inherit;
  }
  .citation-tag:hover { background: var(--accent-glow); color: var(--accent); border-color: var(--accent-dim); }

  /* Input area */
  .chat-input-row {
    display: flex; gap: 8px; padding: 10px 12px;
    border-top: 1px solid var(--border-subtle); align-items: flex-end;
  }
  .chat-input {
    flex: 1; background: var(--bg-base); color: var(--text-primary);
    border: 1px solid var(--border); border-radius: 8px;
    padding: 8px 12px; font-size: 13px; font-family: inherit;
    resize: none; outline: none; transition: border-color 0.15s;
    line-height: 1.5;
  }
  .chat-input:focus { border-color: var(--accent); box-shadow: 0 0 0 2px rgba(124,108,255,0.12); }
  .chat-input-actions { display: flex; flex-direction: column; gap: 4px; align-items: stretch; }
  .send-btn { display: flex; align-items: center; gap: 5px; white-space: nowrap; }
  .retry-btn { align-self: stretch; }
</style>
