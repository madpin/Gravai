<script lang="ts">
  /**
   * Smart app picker with search, categorization, and recent/popular apps on top.
   * Handles 100+ items gracefully with fuzzy search and grouped display.
   */

  let {
    apps = [],
    selected = "",
    onselect = (_v: string) => {},
  }: {
    apps: any[];
    selected: string;
    onselect: (value: string) => void;
  } = $props();

  let query = $state("");
  let open = $state(false);
  let inputEl: HTMLInputElement;

  // Well-known apps that are likely targets for audio capture
  const PRIORITY_APPS: Record<string, number> = {
    "zoom.us": 10, "zoom": 10, "Zoom": 10,
    "Google Chrome": 9, "chrome": 9, "Google Chrome Helper": -1,
    "Safari": 9,
    "Firefox": 8, "firefox": 8,
    "Microsoft Teams": 8, "Teams": 8,
    "Slack": 8,
    "Discord": 8,
    "FaceTime": 7,
    "Spotify": 6,
    "Music": 6, "Apple Music": 6,
    "VLC": 6,
    "Arc": 7,
    "Brave Browser": 7,
    "Microsoft Edge": 7,
    "YouTube Music": 6,
  };

  // Apps to hide (system processes, helpers, daemons)
  const HIDDEN_PATTERNS = [
    /helper/i, /agent/i, /daemon/i, /service/i, /\.xpc$/i,
    /^com\.apple\./, /^kernel/, /^launchd/, /^syslog/,
    /^mdworker/, /^mds/, /^configd/, /^opendirectory/,
    /^loginwindow/, /^WindowServer/, /^coreaudio/i,
    /^bluetoothd/, /^airportd/, /^wifid/, /^nsurlsession/i,
    /^trustd/, /^securityd/, /^syspolicyd/, /^sandboxd/,
  ];

  function isHidden(name: string): boolean {
    return HIDDEN_PATTERNS.some(p => p.test(name));
  }

  function getPriority(name: string): number {
    // Exact match first
    if (PRIORITY_APPS[name] !== undefined) return PRIORITY_APPS[name];
    // Partial match
    for (const [key, score] of Object.entries(PRIORITY_APPS)) {
      if (name.toLowerCase().includes(key.toLowerCase())) return score;
    }
    return 0;
  }

  // Filter, score, and sort apps
  function filteredApps(): { name: string; id: string; priority: number }[] {
    const q = query.toLowerCase().trim();
    const seen = new Set<string>();

    return apps
      .map(a => ({
        name: (a.name || "").trim(),
        id: a.bundle_id || a.name || "",
      }))
      .filter(a => {
        if (!a.name || isHidden(a.name)) return false;
        if (seen.has(a.name)) return false;
        seen.add(a.name);
        if (q && !a.name.toLowerCase().includes(q)) return false;
        return true;
      })
      .map(a => ({ ...a, priority: getPriority(a.name) }))
      .sort((a, b) => {
        // Priority first (descending), then alphabetical
        if (b.priority !== a.priority) return b.priority - a.priority;
        return a.name.localeCompare(b.name);
      })
      .slice(0, 40); // Cap visible items
  }

  function select(id: string, name: string) {
    onselect(id);
    query = id ? name : "";
    open = false;
  }

  function handleFocus() {
    open = true;
  }

  function handleBlur() {
    // Delay to allow click on dropdown items
    setTimeout(() => { open = false; }, 200);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") { open = false; inputEl?.blur(); }
  }

  // Show selected app name
  let displayName = $derived(
    selected ? (apps.find(a => (a.bundle_id || a.name) === selected)?.name || selected) : ""
  );
</script>

<div class="app-picker">
  <input
    bind:this={inputEl}
    class="app-picker-input"
    type="text"
    placeholder={selected ? displayName : "All system audio"}
    bind:value={query}
    onfocus={handleFocus}
    onblur={handleBlur}
    onkeydown={handleKeydown}
  />
  {#if selected && !open}
    <button class="app-picker-clear" onclick={() => select("", "")} title="Clear selection">&times;</button>
  {/if}

  {#if open}
    <div class="app-picker-dropdown">
      <div class="app-picker-item all-apps" role="option" tabindex="-1" aria-selected={!selected} onclick={() => select("", "")} onkeydown={(e) => { if (e.key === "Enter") select("", ""); }}>
        🔊 All system audio
      </div>

      {#each filteredApps() as app}
        <div
          class="app-picker-item"
          class:selected={selected === app.id}
          class:priority={app.priority > 5}
          role="option"
          tabindex="-1"
          aria-selected={selected === app.id}
          onclick={() => select(app.id, app.name)}
          onkeydown={(e) => { if (e.key === "Enter") select(app.id, app.name); }}
        >
          {#if app.priority >= 9}🌟{:else if app.priority >= 6}📱{:else}📄{/if}
          {app.name}
        </div>
      {/each}

      {#if filteredApps().length === 0}
        <div class="app-picker-empty">No matching apps found</div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .app-picker {
    position: relative;
    min-width: 180px;
    max-width: 220px;
  }
  .app-picker-input {
    width: 100%;
    background: var(--bg-base);
    color: var(--text-primary);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 5px 28px 5px 10px;
    font-size: 11px;
    font-family: inherit;
    outline: none;
    transition: border-color 0.15s;
  }
  .app-picker-input:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px rgba(124, 108, 255, 0.15);
  }
  .app-picker-input::placeholder { color: var(--text-tertiary); }
  .app-picker-clear {
    position: absolute;
    right: 6px; top: 50%; transform: translateY(-50%);
    background: none; border: none; color: var(--text-tertiary);
    cursor: pointer; font-size: 14px; line-height: 1;
    padding: 2px;
  }
  .app-picker-clear:hover { color: var(--text-primary); }
  .app-picker-dropdown {
    position: absolute;
    top: calc(100% + 4px);
    left: 0; right: 0;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 8px;
    max-height: 260px;
    overflow-y: auto;
    z-index: 100;
    box-shadow: 0 8px 24px rgba(0,0,0,0.4);
  }
  .app-picker-item {
    padding: 6px 12px;
    font-size: 12px;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: 6px;
    transition: background 0.1s;
  }
  .app-picker-item:hover { background: var(--bg-elevated); }
  .app-picker-item.selected { background: rgba(124, 108, 255, 0.15); color: var(--accent); }
  .app-picker-item.priority { font-weight: 500; }
  .app-picker-item.all-apps {
    border-bottom: 1px solid var(--border);
    font-weight: 600;
    color: var(--text-secondary);
  }
  .app-picker-empty {
    padding: 12px;
    text-align: center;
    color: var(--text-tertiary);
    font-size: 11px;
    font-style: italic;
  }
</style>
