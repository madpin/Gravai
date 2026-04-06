// Gravai Frontend — Professional UI
// Communicates with Rust backend via Tauri invoke()

const { invoke } = (window as any).__TAURI__.core;
const { listen } = (window as any).__TAURI__.event;

// =========================================================================
// State
// =========================================================================
let isRecording = false;
let isPaused = false;
let currentSessionId: string | null = null;
let sessionStartTime: number | null = null;
let autoScrollEnabled = true;
let selectedArchiveSession: string | null = null;

// Intervals
let timerInterval: number | null = null;
let transcriptPoll: number | null = null;
let statusPoll: number | null = null;

// =========================================================================
// Navigation
// =========================================================================
document.querySelectorAll('.nav-item').forEach((item) => {
  item.addEventListener('click', () => {
    document.querySelectorAll('.nav-item').forEach((i) => i.classList.remove('active'));
    document.querySelectorAll('.page').forEach((p) => p.classList.remove('active'));
    item.classList.add('active');
    const page = (item as HTMLElement).dataset.page!;
    document.getElementById(`page-${page}`)?.classList.add('active');
    if (page === 'archive') loadArchive();
    if (page === 'presets') loadPresets();
    if (page === 'profiles') loadProfiles();
    if (page === 'shortcuts') loadShortcuts();
    if (page === 'automations') loadAutomations();
    if (page === 'settings') loadSettings();
  });
});

// =========================================================================
// Recording
// =========================================================================
async function startRecording() {
  try {
    const micOn = (document.getElementById('mic-enabled') as HTMLInputElement).checked;
    const sysOn = (document.getElementById('sys-enabled') as HTMLInputElement).checked;
    await invoke('update_config', {
      patch: { audio: { microphone: { enabled: micOn }, system_audio: { enabled: sysOn } } }
    });
    const result = await invoke('start_session');
    isRecording = true; isPaused = false;
    currentSessionId = result.id;
    sessionStartTime = Date.now();
    updateTransport();
    startPolling();
    // Show summary card for the active session
    const summaryCard = document.getElementById('recording-summary-card');
    if (summaryCard) { summaryCard.style.display = ''; }
    document.getElementById('recording-summary-content')!.innerHTML = '<div class="empty-state">Click "Generate Summary" to create a session summary.</div>';
    log(`Recording started: ${result.id}${result.title ? ' — ' + result.title : ''}`);
  } catch (e) { log(`Error: ${e}`); }
}

async function togglePause() {
  try {
    if (isPaused) {
      await invoke('resume_session'); isPaused = false; log('Resumed');
    } else {
      await invoke('pause_session'); isPaused = true; log('Paused');
    }
    updateTransport();
  } catch (e) { log(`Error: ${e}`); }
}

async function stopRecording() {
  try {
    const result = await invoke('stop_session');
    isRecording = false; isPaused = false;
    currentSessionId = null; sessionStartTime = null;
    updateTransport(); stopPolling();
    log(`Stopped: ${result.id} (${fmtDuration(result.duration_seconds)})`);
  } catch (e) { log(`Error: ${e}`); }
}

function updateTransport() {
  const btnRec = document.getElementById('btn-record') as HTMLButtonElement;
  const btnPause = document.getElementById('btn-pause') as HTMLButtonElement;
  const btnStop = document.getElementById('btn-stop') as HTMLButtonElement;
  const badge = document.getElementById('recording-status')!;

  if (isRecording && !isPaused) {
    btnRec.disabled = true; btnRec.classList.add('active');
    btnPause.disabled = false; btnStop.disabled = false;
    badge.textContent = 'Recording'; badge.className = 'status-badge recording';
  } else if (isRecording && isPaused) {
    btnRec.disabled = true; btnRec.classList.remove('active');
    btnPause.disabled = false; btnStop.disabled = false;
    badge.textContent = 'Paused'; badge.className = 'status-badge paused';
  } else {
    btnRec.disabled = false; btnRec.classList.remove('active');
    btnPause.disabled = true; btnStop.disabled = true;
    badge.textContent = 'Idle'; badge.className = 'status-badge idle';
    document.getElementById('session-timer')!.textContent = '';
    // Reset VU meters
    const vuMic = document.getElementById('vu-mic');
    const vuSys = document.getElementById('vu-sys');
    if (vuMic) vuMic.style.width = '0%';
    if (vuSys) vuSys.style.width = '0%';
  }
}

// =========================================================================
// Timer
// =========================================================================
function startTimer() {
  timerInterval = window.setInterval(() => {
    if (!sessionStartTime) return;
    const s = Math.floor((Date.now() - sessionStartTime) / 1000);
    document.getElementById('session-timer')!.textContent = fmtTimer(s);
  }, 250);
}
function stopTimer() { if (timerInterval) { clearInterval(timerInterval); timerInterval = null; } }
function fmtTimer(s: number): string {
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60).toString().padStart(2, '0');
  const sec = (s % 60).toString().padStart(2, '0');
  return h > 0 ? `${h}:${m}:${sec}` : `${m}:${sec}`;
}
function fmtDuration(s: number): string {
  if (s < 60) return `${s.toFixed(0)}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${Math.floor(s % 60)}s`;
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
}

// =========================================================================
// Polling
// =========================================================================
function startPolling() {
  startTimer();
  document.getElementById('transcript-list')!.innerHTML = '';
  transcriptPoll = window.setInterval(pollTranscript, 1500);
  statusPoll = window.setInterval(pollMeetings, 8000);
}
function stopPolling() {
  stopTimer();
  if (transcriptPoll) { clearInterval(transcriptPoll); transcriptPoll = null; }
}

async function pollTranscript() {
  if (!currentSessionId) return;
  try {
    const utterances = await invoke('get_transcript', { sessionId: currentSessionId });
    renderTranscript(document.getElementById('transcript-list')!, utterances);
  } catch (_) {}
}

// =========================================================================
// Transcript Rendering
// =========================================================================
function sourceIcon(source: string): string {
  if (source === 'microphone' || source === 'mic') return '\u{1F3A4}'; // 🎤
  if (source === 'system_audio' || source === 'system' || source === 'sys') return '\u{1F4BB}'; // 💻
  return '\u{1F50A}'; // 🔊
}

function renderTranscript(el: HTMLElement, utterances: any[]) {
  const scrolledToBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
  el.innerHTML = '';
  if (utterances.length === 0) {
    el.innerHTML = '<div class="empty-state">No transcript yet.</div>';
    return;
  }
  // Assign consistent colors to speakers
  const speakerColors = ['#7c6cff', '#34d399', '#fbbf24', '#f87171', '#60a5fa', '#a78bfa', '#fb923c', '#2dd4bf'];
  const speakerColorMap: Record<string, string> = {};
  let colorIdx = 0;

  for (const u of utterances) {
    const line = document.createElement('div');
    line.className = 'transcript-line';
    const time = new Date(u.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
    const icon = sourceIcon(u.source);
    const conf = u.confidence != null && u.confidence < 0.5 ? ' low-confidence' : '';

    let speakerHtml = '';
    if (u.speaker) {
      if (!speakerColorMap[u.speaker]) {
        speakerColorMap[u.speaker] = speakerColors[colorIdx % speakerColors.length];
        colorIdx++;
      }
      const color = speakerColorMap[u.speaker];
      speakerHtml = ` <span class="speaker-tag" style="color:${color}">${escapeHtml(u.speaker)}</span>`;
    }

    line.innerHTML = `<span class="transcript-meta">${icon} ${time}${speakerHtml}</span><span class="transcript-text${conf}">${escapeHtml(u.text)}</span>`;
    el.appendChild(line);
  }
  if (autoScrollEnabled && scrolledToBottom) el.scrollTop = el.scrollHeight;
}

// =========================================================================
// Volume sliders
// =========================================================================
function wireSlider(sliderId: string, labelId: string) {
  const slider = document.getElementById(sliderId) as HTMLInputElement;
  const label = document.getElementById(labelId)!;
  slider.addEventListener('input', () => { label.textContent = `${slider.value}%`; });
}

// =========================================================================
// Meeting Detection
// =========================================================================
async function pollMeetings() {
  try {
    const meetings: any[] = await invoke('detect_meetings');
    const banner = document.getElementById('meeting-banner')!;
    if (meetings.length > 0 && !isRecording) {
      const names = meetings.map((m: any) => m.app_name).join(', ');
      document.getElementById('meeting-banner-text')!.textContent = `Meeting detected: ${names}`;
      banner.style.display = 'flex';
    } else {
      banner.style.display = 'none';
    }
  } catch (_) {}
}

// =========================================================================
// Archive
// =========================================================================
async function loadArchive() {
  try {
    const sessions: any[] = await invoke('list_sessions');
    const el = document.getElementById('archive-list')!;
    el.innerHTML = '';
    if (sessions.length === 0) { el.innerHTML = '<div class="empty-state">No sessions yet.</div>'; return; }
    for (const s of sessions) {
      const row = document.createElement('div');
      row.className = `archive-row${selectedArchiveSession === s.id ? ' selected' : ''}`;
      const dur = s.duration_seconds ? fmtDuration(s.duration_seconds) : '—';
      const date = new Date(s.started_at).toLocaleDateString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
      row.innerHTML = `<strong>${escapeHtml(s.title || s.id)}</strong><span class="archive-meta">${date} &middot; ${dur} &middot; ${s.meeting_app || ''}</span>`;
      row.addEventListener('click', () => selectSession(s));
      el.appendChild(row);
    }
  } catch (e) { log(`Archive: ${e}`); }
}

async function selectSession(s: any) {
  selectedArchiveSession = s.id;
  // Update header
  const header = document.getElementById('archive-header')!;
  header.textContent = s.title || s.id;
  // Show actions bar
  const actionsBar = document.getElementById('archive-actions');
  if (actionsBar) actionsBar.style.display = 'flex';
  // Reset summary
  const summaryEl = document.getElementById('archive-summary');
  if (summaryEl) { summaryEl.style.display = 'none'; summaryEl.innerHTML = ''; }
  // Load export formats
  loadExportFormats();
  // Highlight selected row
  document.querySelectorAll('.archive-row').forEach(r => r.classList.remove('selected'));
  (event?.target as HTMLElement)?.closest('.archive-row')?.classList.add('selected');
  // Load transcript
  try {
    const utterances = await invoke('get_transcript', { sessionId: s.id });
    renderTranscript(document.getElementById('archive-transcript')!, utterances);
  } catch (e) { log(`Transcript: ${e}`); }
}

// =========================================================================
// Search
// =========================================================================
let searchDebounce: number | null = null;
async function handleSearch(query: string) {
  if (!query.trim()) { loadArchive(); return; }
  try {
    const results: any[] = await invoke('search_utterances', { query });
    const el = document.getElementById('archive-transcript')!;
    renderTranscript(el, results);
    document.getElementById('archive-header')!.textContent = `Search: "${query}" (${results.length} results)`;
  } catch (_) {}
}

// =========================================================================
// Presets
// =========================================================================
let activePresetId: string | null = null;

async function loadPresets() {
  try {
    const result = await invoke('get_presets');
    const presets = result.presets || {};
    activePresetId = result.active_preset_id || null;
    const el = document.getElementById('presets-list')!;
    el.innerHTML = '';
    const ids = Object.keys(presets);
    if (ids.length === 0) { el.innerHTML = '<div class="empty-state">No presets configured.</div>'; return; }
    for (const id of ids) {
      const p = presets[id];
      const isActive = id === activePresetId;
      const card = document.createElement('div');
      card.className = `card preset-card${isActive ? ' card-active' : ''}`;
      card.innerHTML = `
        <div class="card-header">${escapeHtml(p.name || id)}${isActive ? ' <span class="status-badge recording">Active</span>' : ''}</div>
        <div class="card-body">
          <p class="card-desc">${escapeHtml(p.description || '')}</p>
          <div class="card-meta">
            <span>Mic: ${p.mic_enabled ? 'On' : 'Off'}</span>
            <span>System: ${p.sys_enabled ? 'On' : 'Off'}</span>
            <span>Rate: ${p.sample_rate || '—'} Hz</span>
            <span>Format: ${p.format || '—'}</span>
          </div>
        </div>
        <div class="card-footer">
          <button class="btn btn-xs ${isActive ? 'btn-ghost' : 'btn-accent'}" data-preset-id="${escapeHtml(id)}" ${isActive ? 'disabled' : ''}>${isActive ? 'Active' : 'Activate'}</button>
        </div>`;
      card.querySelector('button[data-preset-id]')?.addEventListener('click', () => activatePreset(id));
      el.appendChild(card);
    }
  } catch (e) { log(`Presets: ${e}`); }
}

async function activatePreset(presetId: string) {
  try {
    await invoke('activate_preset', { presetId });
    log(`Preset activated: ${presetId}`);
    await loadPresets();
  } catch (e) { log(`Activate preset: ${e}`); }
}

// =========================================================================
// Profiles
// =========================================================================
let activeProfileId: string | null = null;

async function loadProfiles() {
  try {
    const result = await invoke('get_profiles');
    const profiles = result.profiles || {};
    activeProfileId = result.active_profile_id || null;
    const el = document.getElementById('profiles-list')!;
    el.innerHTML = '';
    const ids = Object.keys(profiles);
    if (ids.length === 0) { el.innerHTML = '<div class="empty-state">No profiles configured.</div>'; return; }
    for (const id of ids) {
      const p = profiles[id];
      const isActive = id === activeProfileId;
      const card = document.createElement('div');
      card.className = `card profile-card${isActive ? ' card-active' : ''}`;
      card.innerHTML = `
        <div class="card-header">${escapeHtml(p.name || id)}${isActive ? ' <span class="status-badge recording">Active</span>' : ''}</div>
        <div class="card-body">
          <p class="card-desc">${escapeHtml(p.description || '')}</p>
          <div class="card-meta">
            <span>Preset: ${escapeHtml(p.linked_preset || '—')}</span>
            ${p.overrides ? `<span>Overrides: ${escapeHtml(JSON.stringify(p.overrides))}</span>` : ''}
          </div>
        </div>
        <div class="card-footer">
          <button class="btn btn-xs ${isActive ? 'btn-ghost' : 'btn-accent'}" data-profile-id="${escapeHtml(id)}" ${isActive ? 'disabled' : ''}>${isActive ? 'Active' : 'Activate'}</button>
        </div>`;
      card.querySelector('button[data-profile-id]')?.addEventListener('click', () => activateProfile(id));
      el.appendChild(card);
    }
  } catch (e) { log(`Profiles: ${e}`); }
}

async function activateProfile(profileId: string) {
  try {
    await invoke('activate_profile', { profileId });
    log(`Profile activated: ${profileId}`);
    await loadProfiles();
  } catch (e) { log(`Activate profile: ${e}`); }
}

// =========================================================================
// Shortcuts
// =========================================================================
async function loadShortcuts() {
  try {
    const result = await invoke('get_shortcuts');
    const bindings = result.bindings || {};
    const tbody = document.getElementById('shortcuts-tbody')!;
    tbody.innerHTML = '';
    const actionIds = Object.keys(bindings);
    if (actionIds.length === 0) {
      tbody.innerHTML = '<tr><td colspan="5" class="empty-state">No shortcuts configured.</td></tr>';
      return;
    }
    for (const actionId of actionIds) {
      const b = bindings[actionId];
      const tr = document.createElement('tr');
      tr.innerHTML = `
        <td><code>${escapeHtml(b.action_id)}</code></td>
        <td>${escapeHtml(b.description || '')}</td>
        <td><input type="text" class="input input-narrow shortcut-key-input" value="${escapeHtml(b.key_sequence || '')}" data-action-id="${escapeHtml(actionId)}" /></td>
        <td><label class="toggle-label"><input type="checkbox" class="toggle shortcut-global-toggle" ${b.is_global ? 'checked' : ''} data-action-id="${escapeHtml(actionId)}" /> Global</label></td>
        <td><button class="btn btn-xs btn-accent shortcut-save-btn" data-action-id="${escapeHtml(actionId)}">Save</button></td>`;
      tr.querySelector('.shortcut-save-btn')?.addEventListener('click', () => {
        const keyInput = tr.querySelector('.shortcut-key-input') as HTMLInputElement;
        rebindShortcut(actionId, keyInput.value);
      });
      tbody.appendChild(tr);
    }
  } catch (e) { log(`Shortcuts: ${e}`); }
}

async function rebindShortcut(actionId: string, keySequence: string) {
  try {
    await invoke('rebind_shortcut', { actionId, keySequence });
    log(`Shortcut rebound: ${actionId} -> ${keySequence}`);
  } catch (e) { log(`Rebind shortcut: ${e}`); }
}

// =========================================================================
// Automations
// =========================================================================
async function loadAutomations() {
  try {
    const result = await invoke('get_automations');
    const automations = result.automations || {};
    const el = document.getElementById('automations-list')!;
    el.innerHTML = '';
    const ids = Object.keys(automations);
    if (ids.length === 0) { el.innerHTML = '<div class="empty-state">No automations configured.</div>'; return; }
    for (const id of ids) {
      const a = automations[id];
      const card = document.createElement('div');
      card.className = 'card automation-card';
      const lastRun = a.last_run ? new Date(a.last_run).toLocaleString() : 'Never';
      card.innerHTML = `
        <div class="card-header">
          ${escapeHtml(a.name || id)}
          <label class="switch"><input type="checkbox" class="automation-toggle" data-automation-id="${escapeHtml(id)}" ${a.enabled ? 'checked' : ''} /><span class="switch-slider"></span></label>
        </div>
        <div class="card-body">
          <div class="card-meta">
            <span>Trigger: ${escapeHtml(a.trigger || '—')}</span>
            <span>Runs: ${a.run_count ?? 0}</span>
            <span>Last: ${lastRun}</span>
          </div>
        </div>`;
      card.querySelector('.automation-toggle')?.addEventListener('change', (e) => {
        const enabled = (e.target as HTMLInputElement).checked;
        toggleAutomation(id, enabled);
      });
      el.appendChild(card);
    }
  } catch (e) { log(`Automations: ${e}`); }
}

async function toggleAutomation(automationId: string, enabled: boolean) {
  try {
    await invoke('toggle_automation', { automationId, enabled });
    log(`Automation ${automationId}: ${enabled ? 'enabled' : 'disabled'}`);
  } catch (e) { log(`Toggle automation: ${e}`); }
}

// =========================================================================
// Summary
// =========================================================================
async function generateSummary(sessionId: string, targetEl: HTMLElement) {
  targetEl.innerHTML = '<div class="empty-state">Generating summary...</div>';
  try {
    const summary = await invoke('summarize_session', { sessionId });
    let html = '';
    if (summary.tldr) html += `<div class="summary-section"><h4>TL;DR</h4><p>${escapeHtml(summary.tldr)}</p></div>`;
    if (summary.key_decisions && summary.key_decisions.length > 0) {
      html += `<div class="summary-section"><h4>Key Decisions</h4><ul>${summary.key_decisions.map((d: string) => `<li>${escapeHtml(d)}</li>`).join('')}</ul></div>`;
    }
    if (summary.action_items && summary.action_items.length > 0) {
      html += `<div class="summary-section"><h4>Action Items</h4><ul>${summary.action_items.map((i: string) => `<li>${escapeHtml(i)}</li>`).join('')}</ul></div>`;
    }
    if (summary.open_questions && summary.open_questions.length > 0) {
      html += `<div class="summary-section"><h4>Open Questions</h4><ul>${summary.open_questions.map((q: string) => `<li>${escapeHtml(q)}</li>`).join('')}</ul></div>`;
    }
    targetEl.innerHTML = html || '<div class="empty-state">No summary content returned.</div>';
  } catch (e) {
    targetEl.innerHTML = `<div class="empty-state">Summary failed: ${escapeHtml(String(e))}</div>`;
    log(`Summary: ${e}`);
  }
}

// =========================================================================
// Export Audio
// =========================================================================
async function loadExportFormats() {
  try {
    const formats: string[] = await invoke('get_export_formats');
    const select = document.getElementById('archive-export-format') as HTMLSelectElement;
    select.innerHTML = '<option value="">Export format...</option>';
    for (const f of formats) {
      const opt = document.createElement('option');
      opt.value = f;
      opt.textContent = f.toUpperCase();
      select.appendChild(opt);
    }
  } catch (_) {}
}

async function exportAudio(sessionId: string, format: string) {
  if (!format) { log('Select an export format first'); return; }
  try {
    await invoke('export_session_audio', { sessionId, format });
    log(`Exported session ${sessionId} as ${format}`);
  } catch (e) { log(`Export: ${e}`); }
}

// =========================================================================
// Settings
// =========================================================================
async function loadSettings() {
  try {
    const config = await invoke('get_config');
    populateSettingsForm(config);
    (document.getElementById('config-editor') as HTMLTextAreaElement).value = JSON.stringify(config, null, 2);
    await loadHealth();
  } catch (e) { log(`Settings: ${e}`); }
}

function populateSettingsForm(c: any) {
  setVal('s-sample-rate', c.audio?.recording?.sample_rate);
  setChecked('s-recording-enabled', c.audio?.recording?.enabled);
  setVal('s-output-folder', c.audio?.recording?.output_folder || '');
  setVal('s-bit-depth', c.audio?.recording?.bit_depth);
  setVal('s-channels', c.audio?.recording?.channels);
  setVal('s-export-format', c.audio?.recording?.export_format);
  setVal('s-aac-bitrate', c.audio?.recording?.aac_bitrate_kbps);
  setVal('s-trans-engine', c.transcription?.engine);
  setVal('s-trans-model', c.transcription?.model);
  setVal('s-trans-lang', c.transcription?.language);
  setVal('s-vad-engine', c.vad?.engine);
  setVal('s-vad-pause', c.vad?.pause_seconds);
  setVal('s-vad-min', c.vad?.silero?.min_utterance_seconds);
  setVal('s-vad-max', c.vad?.silero?.max_utterance_seconds);
  setChecked('s-echo-enabled', c.features?.echo_suppression?.enabled);
  setVal('s-echo-threshold', c.features?.echo_suppression?.similarity_threshold);
  setChecked('s-meeting-enabled', c.features?.meeting_detection?.enabled);
  setChecked('s-diarization-enabled', c.features?.diarization?.enabled);
  setVal('s-llm-provider', c.llm?.provider);
  setVal('s-llm-model', c.llm?.model);
  setVal('s-llm-url', c.llm?.base_url);
}

function buildConfigPatch(): any {
  const folder = val('s-output-folder').trim();
  return {
    audio: { recording: {
      enabled: checked('s-recording-enabled'),
      output_folder: folder || null,
      sample_rate: num('s-sample-rate'),
      bit_depth: num('s-bit-depth'),
      channels: num('s-channels'),
      export_format: val('s-export-format'),
      aac_bitrate_kbps: num('s-aac-bitrate'),
    }},
    transcription: {
      engine: val('s-trans-engine'),
      model: val('s-trans-model'),
      language: val('s-trans-lang'),
    },
    vad: {
      engine: val('s-vad-engine'),
      pause_seconds: numF('s-vad-pause'),
      silero: { min_utterance_seconds: numF('s-vad-min'), max_utterance_seconds: numF('s-vad-max') },
    },
    features: {
      echo_suppression: { enabled: checked('s-echo-enabled'), similarity_threshold: numF('s-echo-threshold') },
      meeting_detection: { enabled: checked('s-meeting-enabled') },
      diarization: { enabled: checked('s-diarization-enabled') },
    },
    llm: {
      provider: val('s-llm-provider'),
      model: val('s-llm-model'),
      base_url: val('s-llm-url'),
    },
  };
}

async function saveSettings() {
  try {
    const patch = buildConfigPatch();
    const updated = await invoke('update_config', { patch });
    (document.getElementById('config-editor') as HTMLTextAreaElement).value = JSON.stringify(updated, null, 2);
    log('Settings saved');
  } catch (e) { log(`Save: ${e}`); }
}

async function saveJsonConfig() {
  try {
    const json = (document.getElementById('config-editor') as HTMLTextAreaElement).value;
    const updated = await invoke('import_config', { json });
    populateSettingsForm(updated);
    log('Config imported from JSON');
  } catch (e) { log(`Import: ${e}`); }
}

async function exportConfig() {
  try {
    const json = await invoke('export_config');
    const blob = new Blob([json], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url; a.download = 'gravai-config.json'; a.click();
    URL.revokeObjectURL(url);
    log('Config exported');
  } catch (e) { log(`Export: ${e}`); }
}

async function resetConfig() {
  if (!confirm('Reset all settings to defaults? This cannot be undone.')) return;
  try {
    const defaults = await invoke('import_config', { json: '{}' });
    populateSettingsForm(defaults);
    (document.getElementById('config-editor') as HTMLTextAreaElement).value = JSON.stringify(defaults, null, 2);
    log('Settings reset to defaults');
  } catch (e) { log(`Reset: ${e}`); }
}

// =========================================================================
// Health
// =========================================================================
async function loadHealth() {
  try {
    const report = await invoke('get_health_report');
    const grid = document.getElementById('health-checks')!;
    grid.innerHTML = '';
    for (const check of report.checks) {
      const item = document.createElement('div');
      item.className = 'health-item';
      item.innerHTML = `<div class="dot ${check.status}"></div><span class="label">${check.name}</span><span class="msg">${check.message}</span>`;
      grid.appendChild(item);
    }
    // Update sidebar indicator
    const dot = document.getElementById('health-indicator')!;
    dot.className = `health-dot ${report.overall === 'ok' ? 'green' : report.overall === 'warn' ? 'yellow' : 'red'}`;
    dot.title = `System: ${report.overall}`;
  } catch (_) {}
}

// =========================================================================
// Log
// =========================================================================
function log(msg: string) {
  const el = document.getElementById('activity-log');
  if (!el) return;
  const line = document.createElement('div');
  const t = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
  line.textContent = `[${t}] ${msg}`;
  el.appendChild(line);
  el.scrollTop = el.scrollHeight;
  // Keep last 100 lines
  while (el.children.length > 100) el.removeChild(el.firstChild!);
}

// =========================================================================
// Helpers
// =========================================================================
function val(id: string): string { return (document.getElementById(id) as HTMLInputElement)?.value ?? ''; }
function num(id: string): number { return parseInt(val(id), 10) || 0; }
function numF(id: string): number { return parseFloat(val(id)) || 0; }
function checked(id: string): boolean { return (document.getElementById(id) as HTMLInputElement)?.checked ?? false; }
function setVal(id: string, v: any) { const el = document.getElementById(id) as HTMLInputElement; if (el && v != null) el.value = String(v); }
function setChecked(id: string, v: any) { const el = document.getElementById(id) as HTMLInputElement; if (el) el.checked = !!v; }
function escapeHtml(s: string): string { const d = document.createElement('div'); d.textContent = s; return d.innerHTML; }

// =========================================================================
// Auto-save settings on change
// =========================================================================
function wireSettingsAutoSave() {
  const ids = [
    's-recording-enabled', 's-output-folder',
    's-sample-rate', 's-bit-depth', 's-channels', 's-export-format', 's-aac-bitrate',
    's-trans-engine', 's-trans-model', 's-trans-lang',
    's-vad-engine', 's-vad-pause', 's-vad-min', 's-vad-max',
    's-echo-enabled', 's-echo-threshold', 's-meeting-enabled', 's-diarization-enabled',
    's-llm-provider', 's-llm-model', 's-llm-url',
  ];
  for (const id of ids) {
    document.getElementById(id)?.addEventListener('change', saveSettings);
  }
}

// =========================================================================
// Tauri Event Listeners (real-time from Rust EventBus)
// =========================================================================
async function wireEventListeners() {
  // VU meter updates — payload shape: { type: "VolumeLevel", data: { source, db } }
  await listen('gravai:volume', (event: any) => {
    const p = event.payload;
    const data = p?.data || p;
    if (!data) return;
    const source: string = data.source ?? '';
    const db: number = data.db ?? -100;
    if (!source) return;
    // Convert dB to percentage (-60dB=0%, 0dB=100%)
    const pct = Math.max(0, Math.min(100, ((db + 60) / 60) * 100));
    const id = (source === 'microphone' || source === 'mic') ? 'vu-mic' : 'vu-sys';
    const el = document.getElementById(id);
    if (el) el.style.width = `${pct}%`;
  });

  // Real-time transcript updates — payload: { type: "TranscriptUpdated", data: { source, text, timestamp, ... } }
  await listen('gravai:transcript', (event: any) => {
    const p = event.payload;
    const data = p?.data || p;
    if (!data || !currentSessionId) return;
    // Append single utterance without re-fetching all
    const el = document.getElementById('transcript-list');
    if (!el) return;
    // Remove empty state placeholder
    const empty = el.querySelector('.empty-state');
    if (empty) empty.remove();

    const line = document.createElement('div');
    line.className = 'transcript-line';
    const source = data.source || '';
    const text = data.text || '';
    const timestamp = data.timestamp || new Date().toISOString();
    const time = new Date(timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
    const icon = sourceIcon(source);
    line.innerHTML = `<span class="transcript-meta">${icon} ${time}</span><span class="transcript-text">${escapeHtml(text)}</span>`;
    el.appendChild(line);
    if (autoScrollEnabled) el.scrollTop = el.scrollHeight;
  });

  // Error notifications — payload: { type: "Error", data: { message } }
  await listen('gravai:error', (event: any) => {
    const p = event.payload;
    const data = p?.data || p;
    const msg = data?.message || 'Unknown error';
    log(`\u26A0\uFE0F ${msg}`);
  });

  // Session state changes
  await listen('gravai:session', (_event: any) => {
    // Could update UI state here too
  });
}

// =========================================================================
// Init
// =========================================================================
async function init() {
  try {
    wireSlider('mic-volume', 'mic-volume-label');
    wireSlider('sys-volume', 'sys-volume-label');
    wireSettingsAutoSave();
    await wireEventListeners();
    updateTransport();
    await loadHealth();
    pollMeetings();
    setInterval(pollMeetings, 10000);

    const devices = await invoke('list_audio_devices');
    log(`${devices.length} audio device(s) found`);
    log('Gravai ready');
  } catch (e) {
    console.error(e);
    log(`Init error: ${e}`);
  }
}

// Wire buttons
document.addEventListener('DOMContentLoaded', () => {
  document.getElementById('btn-record')?.addEventListener('click', startRecording);
  document.getElementById('btn-pause')?.addEventListener('click', togglePause);
  document.getElementById('btn-stop')?.addEventListener('click', stopRecording);
  document.getElementById('auto-scroll')?.addEventListener('change', (e) => { autoScrollEnabled = (e.target as HTMLInputElement).checked; });
  document.getElementById('meeting-btn-record')?.addEventListener('click', () => { document.getElementById('meeting-banner')!.style.display = 'none'; startRecording(); });
  document.getElementById('meeting-btn-dismiss')?.addEventListener('click', () => { document.getElementById('meeting-banner')!.style.display = 'none'; });
  document.getElementById('btn-save-json')?.addEventListener('click', saveJsonConfig);
  document.getElementById('btn-export-config')?.addEventListener('click', exportConfig);
  document.getElementById('btn-import-config')?.addEventListener('click', async () => {
    const input = document.createElement('input');
    input.type = 'file'; input.accept = '.json';
    input.onchange = async () => {
      if (!input.files?.length) return;
      const text = await input.files[0].text();
      try {
        const updated = await invoke('import_config', { json: text });
        populateSettingsForm(updated);
        (document.getElementById('config-editor') as HTMLTextAreaElement).value = JSON.stringify(updated, null, 2);
        log('Config imported from file');
      } catch (e) { log(`Import: ${e}`); }
    };
    input.click();
  });
  document.getElementById('btn-reset-config')?.addEventListener('click', resetConfig);
  document.getElementById('archive-search')?.addEventListener('input', (e) => {
    if (searchDebounce) clearTimeout(searchDebounce);
    searchDebounce = window.setTimeout(() => handleSearch((e.target as HTMLInputElement).value), 300);
  });

  // Recording summary
  document.getElementById('btn-generate-summary')?.addEventListener('click', () => {
    if (!currentSessionId) return;
    const el = document.getElementById('recording-summary-content')!;
    generateSummary(currentSessionId, el);
  });

  // Archive summary & export
  document.getElementById('btn-archive-summarize')?.addEventListener('click', () => {
    if (!selectedArchiveSession) return;
    const el = document.getElementById('archive-summary')!;
    el.style.display = 'block';
    generateSummary(selectedArchiveSession, el);
  });
  document.getElementById('btn-archive-export')?.addEventListener('click', () => {
    if (!selectedArchiveSession) return;
    const format = (document.getElementById('archive-export-format') as HTMLSelectElement).value;
    exportAudio(selectedArchiveSession, format);
  });
});

init();
