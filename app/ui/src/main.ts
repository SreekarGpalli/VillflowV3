import { invoke } from "@tauri-apps/api/core";

// --- INTERFACES ---

interface Settings {
  version: number;
  general: {
    launch_at_startup: boolean;
    start_minimized: boolean;
    show_error_notifications: boolean;
  };
  hotkeys: {
    dictation: string;
    command_mode: string;
    scratchpad: string;
  };
  audio: {
    input_device: string;
  };
  stt: {
    api_keys: string[];
    endpoint: string;
    model_id: string;
    language_code: string;
  };
  llm: {
    api_key: string;
    model: string;
    cleanup_level: string;
  };
  prompts: {
    light: string;
    medium: string;
    high: string;
    command: string;
  };
  output: {
    injection_method: string;
    restore_clipboard: boolean;
  };
  dictionary: {
    auto_learn: boolean;
  };
}

interface DictEntry {
  id: number;
  word: string;
  starred: boolean;
  source: string;
  use_count: number;
}

interface HistoryEntry {
  id?: number;
  ts: string;
  app_name: string;
  window_title: string;
  mode: string;
  raw_transcript: string;
  final_text: string;
  duration_ms: number;
  word_count: number;
}

interface InsightsSummary {
  total_words: number;
  avg_wpm: number;
  top_apps: [string, number][];
  daily_words: [string, number][];
}

// --- STATE VARIABLES ---

let savedSettings: Settings | null = null;
let currentSettings: Settings | null = null;
let activeTab = "general";
let editingWordId: number | null = null;

// --- DOM ELEMENTS CACHE ---

const tabItems = document.querySelectorAll(".nav-item");
const tabContents = document.querySelectorAll(".tab-content");
const saveBar = document.getElementById("save-bar");
const saveConfirmBtn = document.getElementById("save-confirm-btn");
const saveDiscardBtn = document.getElementById("save-discard-btn");

// --- INITIALIZE APP ---

async function init() {
  setupTabs();
  setupSettingsChangeListeners();
  setupHotkeyRecorders();
  setupDictionaryHandlers();
  setupElevenLabsHandlers();
  setupGroqHandlers();
  setupPromptResetHandlers();
  
  await loadAudioDevices();
  await loadSettings();
  await loadDictionary();
  await loadHistory();
  await loadInsights();
}

// --- TAB ROUTING ---

function setupTabs() {
  tabItems.forEach(item => {
    item.addEventListener("click", async () => {
      const tab = item.getAttribute("data-tab");
      if (!tab) return;
      
      tabItems.forEach(i => i.classList.remove("active"));
      tabContents.forEach(c => c.classList.remove("active"));
      
      item.classList.add("active");
      const targetContent = document.getElementById(`tab-${tab}`);
      if (targetContent) targetContent.classList.add("active");
      
      activeTab = tab;
      
      // Load specific tab data on switch
      if (tab === "dictionary") {
        await loadDictionary();
      } else if (tab === "history") {
        await loadHistory();
      } else if (tab === "insights") {
        await loadInsights();
      }
    });
  });
}

// --- SETTINGS LOAD / SAVE ---

async function loadSettings() {
  try {
    const settings = await invoke<Settings>("get_settings");
    savedSettings = JSON.parse(JSON.stringify(settings));
    currentSettings = settings;
    populateForm(settings);
    updateSaveBar();
  } catch (err) {
    console.error("Failed to load settings:", err);
  }
}

function populateForm(settings: Settings) {
  // General Tab
  (document.getElementById("gen-launch-at-startup") as HTMLInputElement).checked = settings.general.launch_at_startup;
  (document.getElementById("gen-start-minimized") as HTMLInputElement).checked = settings.general.start_minimized;
  (document.getElementById("gen-show-notifications") as HTMLInputElement).checked = settings.general.show_error_notifications;

  // Dictation Tab
  const deviceSelect = document.getElementById("dict-audio-device") as HTMLSelectElement;
  deviceSelect.value = settings.audio.input_device;
  
  const cleanups = document.getElementsByName("dict-cleanup");
  cleanups.forEach(radio => {
    const input = radio as HTMLInputElement;
    input.checked = (input.value === settings.llm.cleanup_level);
  });

  // Hotkeys Tab
  (document.getElementById("hk-dictation") as HTMLInputElement).value = settings.hotkeys.dictation;
  (document.getElementById("hk-command") as HTMLInputElement).value = settings.hotkeys.command_mode;
  (document.getElementById("hk-scratchpad") as HTMLInputElement).value = settings.hotkeys.scratchpad;

  // Dictionary Tab
  (document.getElementById("dict-auto-learn") as HTMLInputElement).checked = settings.dictionary.auto_learn;

  // AI Services Tab
  renderElevenLabsKeysList(settings.stt.api_keys);
  (document.getElementById("el-endpoint") as HTMLSelectElement).value = settings.stt.endpoint;
  (document.getElementById("groq-key") as HTMLInputElement).value = settings.llm.api_key;
  
  // Set selected model
  const modelSelect = document.getElementById("groq-model") as HTMLSelectElement;
  let modelExists = false;
  for (let i = 0; i < modelSelect.options.length; i++) {
    if (modelSelect.options[i].value === settings.llm.model) {
      modelSelect.selectedIndex = i;
      modelExists = true;
      break;
    }
  }
  if (!modelExists && settings.llm.model) {
    const opt = document.createElement("option");
    opt.value = settings.llm.model;
    opt.text = settings.llm.model;
    opt.selected = true;
    modelSelect.add(opt);
  }

  // Prompts Tab
  (document.getElementById("prompt-light-text") as HTMLTextAreaElement).value = settings.prompts.light;
  (document.getElementById("prompt-medium-text") as HTMLTextAreaElement).value = settings.prompts.medium;
  (document.getElementById("prompt-high-text") as HTMLTextAreaElement).value = settings.prompts.high;
  (document.getElementById("prompt-command-text") as HTMLTextAreaElement).value = settings.prompts.command;

  // Output Tab
  const methods = document.getElementsByName("out-method");
  methods.forEach(radio => {
    const input = radio as HTMLInputElement;
    input.checked = (input.value === settings.output.injection_method);
  });
  (document.getElementById("out-restore-clipboard") as HTMLInputElement).checked = settings.output.restore_clipboard;
}

function gatherFormSettings(): Settings {
  const cleanups = document.getElementsByName("dict-cleanup");
  let cleanup_level = "medium";
  cleanups.forEach(radio => {
    const input = radio as HTMLInputElement;
    if (input.checked) cleanup_level = input.value;
  });

  const methods = document.getElementsByName("out-method");
  let injection_method = "clipboard_paste";
  methods.forEach(radio => {
    const input = radio as HTMLInputElement;
    if (input.checked) injection_method = input.value;
  });

  const deviceSelect = document.getElementById("dict-audio-device") as HTMLSelectElement;
  const modelSelect = document.getElementById("groq-model") as HTMLSelectElement;

  return {
    version: 1,
    general: {
      launch_at_startup: (document.getElementById("gen-launch-at-startup") as HTMLInputElement).checked,
      start_minimized: (document.getElementById("gen-start-minimized") as HTMLInputElement).checked,
      show_error_notifications: (document.getElementById("gen-show-notifications") as HTMLInputElement).checked
    },
    hotkeys: {
      dictation: (document.getElementById("hk-dictation") as HTMLInputElement).value,
      command_mode: (document.getElementById("hk-command") as HTMLInputElement).value,
      scratchpad: (document.getElementById("hk-scratchpad") as HTMLInputElement).value
    },
    audio: {
      input_device: deviceSelect.value
    },
    stt: {
      api_keys: currentSettings?.stt.api_keys || [],
      endpoint: (document.getElementById("el-endpoint") as HTMLSelectElement).value,
      model_id: currentSettings?.stt.model_id || "scribe_v2_realtime",
      language_code: currentSettings?.stt.language_code || "en"
    },
    llm: {
      api_key: (document.getElementById("groq-key") as HTMLInputElement).value,
      model: modelSelect.value,
      cleanup_level
    },
    prompts: {
      light: (document.getElementById("prompt-light-text") as HTMLTextAreaElement).value,
      medium: (document.getElementById("prompt-medium-text") as HTMLTextAreaElement).value,
      high: (document.getElementById("prompt-high-text") as HTMLTextAreaElement).value,
      command: (document.getElementById("prompt-command-text") as HTMLTextAreaElement).value
    },
    output: {
      injection_method,
      restore_clipboard: (document.getElementById("out-restore-clipboard") as HTMLInputElement).checked
    },
    dictionary: {
      auto_learn: (document.getElementById("dict-auto-learn") as HTMLInputElement).checked
    }
  };
}

function setupSettingsChangeListeners() {
  const inputs = document.querySelectorAll("input, select, textarea");
  inputs.forEach(input => {
    input.addEventListener("input", () => {
      currentSettings = gatherFormSettings();
      updateSaveBar();
    });
    input.addEventListener("change", () => {
      currentSettings = gatherFormSettings();
      updateSaveBar();
    });
  });

  saveConfirmBtn?.addEventListener("click", async () => {
    if (!currentSettings || !savedSettings) return;
    try {
      // Autostart registry handler
      if (currentSettings.general.launch_at_startup !== savedSettings.general.launch_at_startup) {
        await invoke("set_autostart", { enabled: currentSettings.general.launch_at_startup });
      }
      
      await invoke("save_settings", { settings: currentSettings });
      savedSettings = JSON.parse(JSON.stringify(currentSettings));
      updateSaveBar();
    } catch (err) {
      alert(`Failed to save settings: ${err}`);
    }
  });

  saveDiscardBtn?.addEventListener("click", () => {
    if (!savedSettings) return;
    currentSettings = JSON.parse(JSON.stringify(savedSettings));
    populateForm(currentSettings!);
    updateSaveBar();
  });
}

function updateSaveBar() {
  if (!saveBar) return;
  const isDirty = JSON.stringify(savedSettings) !== JSON.stringify(currentSettings);
  if (isDirty) {
    saveBar.classList.add("active");
  } else {
    saveBar.classList.remove("active");
  }
}

// --- AUDIO HARDWARE ENUMERATION ---

async function loadAudioDevices() {
  try {
    const devices = await invoke<string[]>("list_input_devices");
    const deviceSelect = document.getElementById("dict-audio-device") as HTMLSelectElement;
    
    // Clear dynamic devices, keeping system_default
    while (deviceSelect.options.length > 1) {
      deviceSelect.remove(1);
    }

    devices.forEach(device => {
      if (device !== "system_default") {
        const opt = document.createElement("option");
        opt.value = device;
        opt.text = device;
        deviceSelect.add(opt);
      }
    });
  } catch (err) {
    console.error("Failed to load audio devices:", err);
  }
}

// --- HOTKEY CAPTURING (MODIFIER+KEY REGISTRATION) ---

function setupHotkeyRecorders() {
  const hkInputs = ["hk-dictation", "hk-command", "hk-scratchpad"];
  hkInputs.forEach(id => {
    const input = document.getElementById(id) as HTMLInputElement;
    if (!input) return;

    input.addEventListener("focus", () => {
      input.placeholder = "Listening... Press Keys";
      input.value = "";
    });

    input.addEventListener("blur", () => {
      if (!input.value && savedSettings) {
        // Revert to original
        const key = id === "hk-dictation" ? "dictation" : id === "hk-command" ? "command_mode" : "scratchpad";
        input.value = (savedSettings.hotkeys as any)[key];
      }
      input.placeholder = "Press shortcut keys...";
    });

    input.addEventListener("keydown", (e: KeyboardEvent) => {
      e.preventDefault();
      
      // Ignore alone modifier presses
      if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) {
        return;
      }

      const parts: string[] = [];
      if (e.ctrlKey) parts.push("Ctrl");
      if (e.shiftKey) parts.push("Shift");
      if (e.altKey) parts.push("Alt");
      if (e.metaKey) parts.push("Win");

      let key = e.key.toUpperCase();
      if (key === " ") key = "SPACE";
      
      // Map arrow keys and special keys
      if (key === "ARROWUP") key = "UP";
      if (key === "ARROWDOWN") key = "DOWN";
      if (key === "ARROWLEFT") key = "LEFT";
      if (key === "ARROWRIGHT") key = "RIGHT";

      parts.push(key);
      input.value = parts.join("+");
      input.blur();

      // Trigger change
      currentSettings = gatherFormSettings();
      updateSaveBar();
    });
  });
}

// --- SPELLING DICTIONARY CRUD ---

const dictTableBody = document.getElementById("dict-table-body");
const addWordBtn = document.getElementById("dict-add-btn");
const wordModal = document.getElementById("word-modal");
const wordModalTitle = document.getElementById("word-modal-title");
const wordModalInput = document.getElementById("word-modal-input") as HTMLInputElement;
const wordModalSave = document.getElementById("word-modal-save");
const wordModalCancel = document.getElementById("word-modal-cancel");

function setupDictionaryHandlers() {
  addWordBtn?.addEventListener("click", () => {
    editingWordId = null;
    if (wordModalTitle) wordModalTitle.innerText = "Add Custom Word";
    if (wordModalInput) wordModalInput.value = "";
    if (wordModalSave) wordModalSave.innerText = "Add";
    wordModal?.classList.add("active");
  });

  wordModalCancel?.addEventListener("click", () => {
    wordModal?.classList.remove("active");
  });

  wordModalSave?.addEventListener("click", async () => {
    if (!wordModalInput) return;
    const word = wordModalInput.value.trim();
    if (!word) return;

    try {
      if (editingWordId !== null) {
        await invoke("dictionary_update", { id: editingWordId, word });
      } else {
        await invoke("dictionary_add", { word, source: "manual" });
      }
      wordModal?.classList.remove("active");
      await loadDictionary();
    } catch (err) {
      alert(`Error saving word: ${err}`);
    }
  });
}

async function loadDictionary() {
  try {
    const list = await invoke<DictEntry[]>("dictionary_list");
    if (!dictTableBody) return;
    dictTableBody.innerHTML = "";

    if (list.length === 0) {
      dictTableBody.innerHTML = `
        <tr>
          <td colspan="5" style="text-align: center; color: var(--text-disabled); font-style: italic;">
            Dictionary is empty. Add a spelling above.
          </td>
        </tr>
      `;
      return;
    }

    list.forEach(entry => {
      const row = document.createElement("tr");
      row.innerHTML = `
        <td><strong>${escapeHtml(entry.word)}</strong></td>
        <td><span style="font-size:12px; padding:2px 6px; border-radius:4px; background:rgba(255,255,255,0.05);">${entry.source}</span></td>
        <td>${entry.use_count}</td>
        <td>
          <span class="star-btn" style="cursor:pointer; font-size:16px; color: ${entry.starred ? '#eab308' : 'var(--text-disabled)'};" data-id="${entry.id}">
            ${entry.starred ? '&#9733;' : '&#9734;'}
          </span>
        </td>
        <td>
          <div style="display:flex; gap:6px;">
            <button class="btn btn-sm edit-word" data-id="${entry.id}" data-word="${escapeHtml(entry.word)}">Edit</button>
            <button class="btn btn-sm btn-danger delete-word" data-id="${entry.id}">Delete</button>
          </div>
        </td>
      `;
      dictTableBody.appendChild(row);
    });

    // Wire table button actions
    document.querySelectorAll(".star-btn").forEach(btn => {
      btn.addEventListener("click", async () => {
        const id = parseInt(btn.getAttribute("data-id") || "0");
        try {
          await invoke("dictionary_toggle_star", { id });
          await loadDictionary();
        } catch (err) {
          console.error(err);
        }
      });
    });

    document.querySelectorAll(".edit-word").forEach(btn => {
      btn.addEventListener("click", () => {
        const id = parseInt(btn.getAttribute("data-id") || "0");
        const word = btn.getAttribute("data-word") || "";
        editingWordId = id;
        if (wordModalTitle) wordModalTitle.innerText = "Edit Custom Word";
        if (wordModalInput) wordModalInput.value = word;
        if (wordModalSave) wordModalSave.innerText = "Update";
        wordModal?.classList.add("active");
      });
    });

    document.querySelectorAll(".delete-word").forEach(btn => {
      btn.addEventListener("click", async () => {
        const id = parseInt(btn.getAttribute("data-id") || "0");
        if (confirm("Are you sure you want to delete this custom word?")) {
          try {
            await invoke("dictionary_delete", { id });
            await loadDictionary();
          } catch (err) {
            console.error(err);
          }
        }
      });
    });

  } catch (err) {
    console.error("Failed to load dictionary:", err);
  }
}

// --- ELEVENLABS ORDERED API KEY HIERARCHY ---

const elKeysList = document.getElementById("el-keys-list");
const elNewKeyInput = document.getElementById("el-new-key") as HTMLInputElement;
const elAddKeyBtn = document.getElementById("el-add-key-btn");

function setupElevenLabsHandlers() {
  elAddKeyBtn?.addEventListener("click", () => {
    if (!elNewKeyInput || !currentSettings) return;
    const key = elNewKeyInput.value.trim();
    if (!key) return;

    currentSettings.stt.api_keys.push(key);
    elNewKeyInput.value = "";
    renderElevenLabsKeysList(currentSettings.stt.api_keys);
    updateSaveBar();
  });
}

function renderElevenLabsKeysList(keys: string[]) {
  if (!elKeysList) return;
  elKeysList.innerHTML = "";

  if (keys.length === 0) {
    elKeysList.innerHTML = `
      <div style="font-size: 13px; color: var(--text-disabled); font-style: italic; padding: 4px 0;">
        No API keys added. Reverts to default free limits.
      </div>
    `;
    return;
  }

  keys.forEach((key, idx) => {
    const masked = key.length > 8 ? `${key.substring(0, 4)}••••${key.substring(key.length - 4)}` : "••••••••";
    const row = document.createElement("div");
    row.className = "key-row";
    row.innerHTML = `
      <span class="key-index">${idx + 1}</span>
      <span class="key-value">${masked}</span>
      <div class="key-actions">
        <button class="btn btn-sm move-key-up" data-idx="${idx}" ${idx === 0 ? 'disabled' : ''}>&uarr;</button>
        <button class="btn btn-sm move-key-down" data-idx="${idx}" ${idx === keys.length - 1 ? 'disabled' : ''}>&darr;</button>
        <button class="btn btn-sm btn-danger remove-key" data-idx="${idx}">Remove</button>
      </div>
    `;
    elKeysList.appendChild(row);
  });

  // Wire hierarchy button actions
  document.querySelectorAll(".move-key-up").forEach(btn => {
    btn.addEventListener("click", () => {
      const idx = parseInt(btn.getAttribute("data-idx") || "0");
      if (idx > 0 && currentSettings) {
        const temp = currentSettings.stt.api_keys[idx];
        currentSettings.stt.api_keys[idx] = currentSettings.stt.api_keys[idx - 1];
        currentSettings.stt.api_keys[idx - 1] = temp;
        renderElevenLabsKeysList(currentSettings.stt.api_keys);
        updateSaveBar();
      }
    });
  });

  document.querySelectorAll(".move-key-down").forEach(btn => {
    btn.addEventListener("click", () => {
      const idx = parseInt(btn.getAttribute("data-idx") || "0");
      if (currentSettings && idx < currentSettings.stt.api_keys.length - 1) {
        const temp = currentSettings.stt.api_keys[idx];
        currentSettings.stt.api_keys[idx] = currentSettings.stt.api_keys[idx + 1];
        currentSettings.stt.api_keys[idx + 1] = temp;
        renderElevenLabsKeysList(currentSettings.stt.api_keys);
        updateSaveBar();
      }
    });
  });

  document.querySelectorAll(".remove-key").forEach(btn => {
    btn.addEventListener("click", () => {
      const idx = parseInt(btn.getAttribute("data-idx") || "0");
      if (currentSettings) {
        currentSettings.stt.api_keys.splice(idx, 1);
        renderElevenLabsKeysList(currentSettings.stt.api_keys);
        updateSaveBar();
      }
    });
  });
}

// --- GROQ MODEL SELECTION ---

const groqRefreshBtn = document.getElementById("groq-refresh-btn");
const groqModelSelect = document.getElementById("groq-model") as HTMLSelectElement;

function setupGroqHandlers() {
  groqRefreshBtn?.addEventListener("click", async () => {
    groqRefreshBtn.innerText = "Loading...";
    try {
      const models = await invoke<string[]>("list_groq_models");
      const currentSelected = groqModelSelect.value;
      
      groqModelSelect.innerHTML = "";
      
      models.forEach(model => {
        const opt = document.createElement("option");
        opt.value = model;
        opt.text = model;
        if (model === currentSelected) {
          opt.selected = true;
        }
        groqModelSelect.add(opt);
      });
      alert("Models updated successfully.");
    } catch (err) {
      alert(`Failed to fetch models: ${err}. Ensure your Groq API key is valid.`);
    } finally {
      groqRefreshBtn.innerText = "Refresh";
    }
  });
}

// --- PROMPTS RESET HANDLERS ---

function setupPromptResetHandlers() {
  document.querySelectorAll("[data-reset-prompt]").forEach(btn => {
    btn.addEventListener("click", async () => {
      const name = btn.getAttribute("data-reset-prompt") || "";
      if (confirm(`Reset ${name} prompt back to default text?`)) {
        try {
          const defaultPrompt = await invoke<string>("reset_prompt", { name });
          const textarea = document.getElementById(`prompt-${name}-text`) as HTMLTextAreaElement;
          if (textarea) {
            textarea.value = defaultPrompt;
            currentSettings = gatherFormSettings();
            updateSaveBar();
          }
        } catch (err) {
          console.error(err);
        }
      }
    });
  });
}

// --- HISTORY LIST & EXPANSIONS ---

const historyContainer = document.getElementById("history-container");

async function loadHistory() {
  try {
    const list = await invoke<HistoryEntry[]>("history_list", { limit: 50, offset: 0 });
    if (!historyContainer) return;
    historyContainer.innerHTML = "";

    if (list.length === 0) {
      historyContainer.innerHTML = `
        <div style="text-align: center; color: var(--text-disabled); font-style: italic; padding: 40px 0;">
          No history items found.
        </div>
      `;
      return;
    }

    list.forEach(entry => {
      const date = new Date(entry.ts);
      const timeStr = date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
      const dateStr = date.toLocaleDateString([], { month: 'short', day: 'numeric' });
      
      const row = document.createElement("div");
      row.className = "history-row";
      row.innerHTML = `
        <div class="history-header">
          <div class="history-meta">
            <span>${dateStr} ${timeStr}</span>
            <span class="app-badge">${escapeHtml(entry.app_name)}</span>
            <span style="text-transform: capitalize; color: var(--accent); font-weight: 600;">${entry.mode}</span>
          </div>
          <div class="history-summary">${escapeHtml(entry.final_text)}</div>
          <span class="expand-arrow" style="font-size:12px; color: var(--text-disabled);">&#9660;</span>
        </div>
        <div class="history-body">
          <div class="transcript-block">
            <div>
              <div class="transcript-label">RAW TRANSCRIPT</div>
              <div class="transcript-box">${escapeHtml(entry.raw_transcript)}</div>
            </div>
            <div>
              <div class="transcript-label">FINAL INJECTED TEXT</div>
              <div class="transcript-box">${escapeHtml(entry.final_text)}</div>
            </div>
          </div>
          <div style="display:flex; justify-content: space-between; align-items:center; font-size:12px; color: var(--text-muted); margin-top: 4px;">
            <div>
              <span>Duration: ${(entry.duration_ms / 1000).toFixed(2)}s</span>
              <span style="margin: 0 8px; color: var(--border-color);">|</span>
              <span>Words: ${entry.word_count}</span>
            </div>
            <button class="btn btn-sm copy-history-text" data-text="${escapeHtml(entry.final_text)}">Copy Injected Text</button>
          </div>
        </div>
      `;
      historyContainer.appendChild(row);
    });

    // Wire expandable accordion logic
    document.querySelectorAll(".history-header").forEach(header => {
      header.addEventListener("click", () => {
        const body = header.nextElementSibling as HTMLDivElement;
        const arrow = header.querySelector(".expand-arrow") as HTMLSpanElement;
        
        const isActive = body.classList.contains("active");
        
        // Close others
        document.querySelectorAll(".history-body").forEach(b => b.classList.remove("active"));
        document.querySelectorAll(".expand-arrow").forEach(a => (a.innerHTML = "&#9660;"));

        if (!isActive) {
          body.classList.add("active");
          arrow.innerHTML = "&#9650;";
        }
      });
    });

    // Wire copy buttons
    document.querySelectorAll(".copy-history-text").forEach(btn => {
      btn.addEventListener("click", async (e) => {
        e.stopPropagation(); // Avoid triggering header click
        const text = btn.getAttribute("data-text") || "";
        try {
          await navigator.clipboard.writeText(text);
          const origText = btn.innerHTML;
          btn.innerHTML = "Copied!";
          setTimeout(() => { btn.innerHTML = origText; }, 1500);
        } catch (err) {
          console.error("Failed to copy text:", err);
        }
      });
    });

  } catch (err) {
    console.error("Failed to load history list:", err);
  }
}

// --- TELEMETRY INSIGHTS ---

async function loadInsights() {
  try {
    const summary = await invoke<InsightsSummary>("insights_summary");
    
    // Total words
    const wordsEl = document.getElementById("insight-total-words");
    if (wordsEl) wordsEl.innerText = summary.total_words.toLocaleString();

    // Average WPM
    const wpmEl = document.getElementById("insight-avg-wpm");
    if (wpmEl) wpmEl.innerText = summary.avg_wpm.toFixed(1);

    // Top 5 apps
    const topAppsContainer = document.getElementById("insight-top-apps");
    if (topAppsContainer) {
      topAppsContainer.innerHTML = "";
      if (summary.top_apps.length === 0) {
        topAppsContainer.innerHTML = `<div style="font-size:13px; color:var(--text-disabled); font-style:italic; text-align:center;">No activity logged yet.</div>`;
      } else {
        const maxVal = Math.max(...summary.top_apps.map(item => item[1]));
        summary.top_apps.forEach(([appName, count]) => {
          const fillWidth = maxVal > 0 ? (count / maxVal) * 100 : 0;
          const appRow = document.createElement("div");
          appRow.className = "app-progress-item";
          appRow.innerHTML = `
            <div class="app-progress-meta">
              <span>${escapeHtml(appName)}</span>
              <span>${count} utterances</span>
            </div>
            <div class="app-progress-bar">
              <div class="app-progress-fill" style="width: ${fillWidth}%"></div>
            </div>
          `;
          topAppsContainer.appendChild(appRow);
        });
      }
    }

    // Heatmap Grid (365 days)
    renderHeatmap(summary.daily_words);

  } catch (err) {
    console.error("Failed to load insights summary:", err);
  }
}

function renderHeatmap(dailyWords: [string, number][]) {
  const grid = document.getElementById("heatmap-grid");
  if (!grid) return;
  grid.innerHTML = "";

  // Convert array to a fast lookup map
  const dataMap = new Map<string, number>();
  dailyWords.forEach(([day, count]) => {
    dataMap.set(day, count);
  });

  const now = new Date();
  const cells: HTMLDivElement[] = [];

  // Generate boxes starting from 365 days ago up to today
  for (let i = 365; i >= 0; i--) {
    const dayDate = new Date(now.getTime() - i * 24 * 60 * 60 * 1000);
    const dayStr = dayDate.toISOString().split("T")[0];
    const wordCount = dataMap.get(dayStr) || 0;

    const cell = document.createElement("div");
    cell.className = "heatmap-cell";
    cell.setAttribute("data-date", dayStr);
    cell.setAttribute("data-count", wordCount.toString());
    
    // Choose color level
    let level = 0;
    if (wordCount > 0 && wordCount <= 100) level = 1;
    else if (wordCount > 100 && wordCount <= 500) level = 2;
    else if (wordCount > 500 && wordCount <= 1000) level = 3;
    else if (wordCount > 1000) level = 4;
    
    if (level > 0) {
      cell.setAttribute("data-level", level.toString());
    }

    cell.title = `${dayStr}: ${wordCount} word${wordCount === 1 ? '' : 's'}`;
    cells.push(cell);
  }

  // To draw a grid of 7 rows (Sunday to Saturday) columns-first,
  // we align cells according to their day-of-week index.
  cells.forEach(cell => {
    grid.appendChild(cell);
  });
}

// --- UTILITIES ---

function escapeHtml(unsafe: string): string {
  return unsafe
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

// --- BOOTSTRAP ---

window.addEventListener("DOMContentLoaded", () => {
  init();
});
