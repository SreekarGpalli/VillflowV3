import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// --- INTERFACES ---

interface Settings {
  version: number;
  general: {
    launch_at_startup: boolean;
    start_minimized: boolean;
    show_error_notifications: boolean;
    history_retention_days: number;
  };
  vault: {
    mode: "dpapi" | "passphrase";
    sealed?: { salt_b64: string; nonce_b64: string; ciphertext_b64: string } | null;
  };
  hotkeys: {
    dictation: string;
    command_mode: string;
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
    include_field_context: boolean;
  };
  prompts: {
    light: string;
    medium: string;
    high: string;
    command: string;
    command_generate: string;
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
let editingWordId: number | null = null;
let historyOffset = 0;
const historyLimit = 15;
let lastEngineError = "(none)";
let lastEngineState = "Idle";
let lastDictationSummary = "(none yet)";

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
  setupEngineEventListeners();
  setupSetupTabHandlers();
  setupVaultHandlers();
  
  await loadAudioDevices();
  await loadSettings();
  await refreshVaultUi();
  await loadDictionary();
  await loadHistory();
  await loadInsights();
  updateReadyChecklist();

  // Wire up Load More button click
  const loadMoreBtn = document.getElementById("history-load-more-btn");
  loadMoreBtn?.addEventListener("click", () => {
    historyOffset += historyLimit;
    loadHistory(true);
  });

  // Wire up Clear All history button
  const clearAllBtn = document.getElementById("history-clear-all-btn");
  clearAllBtn?.addEventListener("click", async () => {
    if (!confirm("Delete ALL history entries? This cannot be undone.")) return;
    try {
      await invoke("history_clear");
      await loadHistory();
      showToast("History cleared.", "success");
    } catch (err) {
      showToast(`Failed to clear history: ${err}`, "error");
    }
  });

  document.getElementById("history-export-btn")?.addEventListener("click", async () => {
    try {
      const json = await invoke<string>("history_export");
      const blob = new Blob([json], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `villflow-history-${new Date().toISOString().slice(0, 10)}.json`;
      a.click();
      URL.revokeObjectURL(url);
      showToast("History exported.", "success");
    } catch (err) {
      showToast(`Export failed: ${err}`, "error");
    }
  });
}

// --- TAB ROUTING ---

function setupTabs() {
  const activateTab = async (item: Element) => {
    const tab = item.getAttribute("data-tab");
    if (!tab) return;

    tabItems.forEach((i) => {
      i.classList.remove("active");
      i.setAttribute("aria-selected", "false");
      (i as HTMLElement).tabIndex = -1;
    });
    tabContents.forEach((c) => c.classList.remove("active"));

    item.classList.add("active");
    item.setAttribute("aria-selected", "true");
    (item as HTMLElement).tabIndex = 0;
    const targetContent = document.getElementById(`tab-${tab}`);
    if (targetContent) targetContent.classList.add("active");

    if (tab === "dictionary") {
      await loadDictionary();
    } else if (tab === "history") {
      await loadHistory();
    } else if (tab === "insights") {
      await loadInsights();
    } else if (tab === "overview") {
      updateReadyChecklist();
    }
  };

  tabItems.forEach((item) => {
    item.addEventListener("click", () => {
      void activateTab(item);
    });
    item.addEventListener("keydown", (e) => {
      const ke = e as KeyboardEvent;
      if (ke.key === "Enter" || ke.key === " ") {
        ke.preventDefault();
        void activateTab(item);
      }
    });
  });
}

// --- SETTINGS LOAD / SAVE ---

async function loadSettings() {
  try {
    const settings = await invoke<Settings>("get_settings");
    // Reconcile launch-at-startup checkbox with actual registry when possible.
    try {
      const regOn = await invoke<boolean>("autostart_status");
      if (settings.general.launch_at_startup !== regOn) {
        // Prefer the settings file as source of truth and re-sync registry later on save.
        // Surface registry state if settings say off but registry is on.
        if (!settings.general.launch_at_startup && regOn) {
          settings.general.launch_at_startup = true;
        }
      }
    } catch {
      /* autostart_status optional */
    }
    savedSettings = JSON.parse(JSON.stringify(settings));
    currentSettings = settings;
    populateForm(settings);
    updateSaveBar();
  } catch (err) {
    console.error("Failed to load settings:", err);
    showToast(`Failed to load settings: ${err}`, "error");
  }
}

function populateForm(settings: Settings) {
  // General Tab
  (document.getElementById("gen-launch-at-startup") as HTMLInputElement).checked = settings.general.launch_at_startup;
  (document.getElementById("gen-start-minimized") as HTMLInputElement).checked = settings.general.start_minimized;
  (document.getElementById("gen-show-notifications") as HTMLInputElement).checked = settings.general.show_error_notifications;
  const retention = document.getElementById("gen-history-retention") as HTMLSelectElement | null;
  if (retention) {
    const d = settings.general.history_retention_days ?? 0;
    retention.value = String([0, 30, 90, 365].includes(d) ? d : 0);
  }

  // Dictation Tab
  const deviceSelect = document.getElementById("dict-audio-device") as HTMLSelectElement;
  // Option values use system_default; label is "System default".
  let devVal = settings.audio.input_device;
  if (devVal === "System default") devVal = "system_default";
  deviceSelect.value = devVal;
  
  const cleanups = document.getElementsByName("dict-cleanup");
  cleanups.forEach(radio => {
    const input = radio as HTMLInputElement;
    input.checked = (input.value === settings.llm.cleanup_level);
  });

  // Hotkeys Tab
  (document.getElementById("hk-dictation") as HTMLInputElement).value = settings.hotkeys.dictation;
  (document.getElementById("hk-command") as HTMLInputElement).value = settings.hotkeys.command_mode;

  // Dictionary Tab
  (document.getElementById("dict-auto-learn") as HTMLInputElement).checked = settings.dictionary.auto_learn;

  // Field context (advanced)
  const ctxToggle = document.getElementById("llm-include-field-context") as HTMLInputElement | null;
  if (ctxToggle) ctxToggle.checked = !!settings.llm.include_field_context;

  // API keys (Setup)
  renderElevenLabsKeysList(settings.stt.api_keys);
  (document.getElementById("el-endpoint") as HTMLSelectElement).value = settings.stt.endpoint;
  (document.getElementById("groq-key") as HTMLInputElement).value = settings.llm.api_key;

  updateHotkeySummaries(settings);
  updateReadyChecklist();
  
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
  (document.getElementById("prompt-command_generate-text") as HTMLTextAreaElement).value = settings.prompts.command_generate;

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

  let inputDevice = deviceSelect.value;
  if (inputDevice === "System default") inputDevice = "system_default";

  return {
    // Keep schema version from loaded settings (engine migrates to current).
    version: currentSettings?.version ?? savedSettings?.version ?? 2,
    general: {
      launch_at_startup: (document.getElementById("gen-launch-at-startup") as HTMLInputElement).checked,
      start_minimized: (document.getElementById("gen-start-minimized") as HTMLInputElement).checked,
      show_error_notifications: (document.getElementById("gen-show-notifications") as HTMLInputElement).checked,
      history_retention_days: parseInt(
        (document.getElementById("gen-history-retention") as HTMLSelectElement | null)?.value || "0",
        10,
      ) || 0,
    },
    hotkeys: {
      dictation: (document.getElementById("hk-dictation") as HTMLInputElement).value,
      command_mode: (document.getElementById("hk-command") as HTMLInputElement).value,
    },
    audio: {
      input_device: inputDevice
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
      cleanup_level,
      include_field_context:
        (document.getElementById("llm-include-field-context") as HTMLInputElement | null)
          ?.checked ?? false,
    },
    prompts: {
      light: (document.getElementById("prompt-light-text") as HTMLTextAreaElement).value,
      medium: (document.getElementById("prompt-medium-text") as HTMLTextAreaElement).value,
      high: (document.getElementById("prompt-high-text") as HTMLTextAreaElement).value,
      command: (document.getElementById("prompt-command-text") as HTMLTextAreaElement).value,
      command_generate: (document.getElementById("prompt-command_generate-text") as HTMLTextAreaElement).value
    },
    output: {
      injection_method,
      restore_clipboard: (document.getElementById("out-restore-clipboard") as HTMLInputElement).checked
    },
    dictionary: {
      auto_learn: (document.getElementById("dict-auto-learn") as HTMLInputElement).checked
    },
    vault: currentSettings?.vault ?? savedSettings?.vault ?? { mode: "dpapi", sealed: null },
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
    currentSettings = gatherFormSettings();
    await saveAndApplySettings(false);
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
  updateReadyChecklist();
  if (currentSettings) updateHotkeySummaries(currentSettings);
}

function updateHotkeySummaries(settings: Settings) {
  const set = (id: string, val: string) => {
    const el = document.getElementById(id);
    if (el) el.textContent = val;
  };
  set("howto-dictation", settings.hotkeys.dictation);
  set("howto-command", settings.hotkeys.command_mode);
}

/** PRODUCT Ready checklist — mirrors engine gate logic for UX. */
function computeReady(settings: Settings | null): {
  ready: boolean;
  el: boolean;
  groq: boolean;
  mic: boolean;
  hotkeys: boolean;
  detail: string;
} {
  if (!settings) {
    return {
      ready: false,
      el: false,
      groq: false,
      mic: false,
      hotkeys: false,
      detail: "Loading settings…",
    };
  }
  const el = settings.stt.api_keys.some((k) => k.trim().length > 0);
  const groq =
    settings.llm.cleanup_level === "none" || settings.llm.api_key.trim().length > 0;
  const mic = true; // system_default always acceptable; capture fails later if no device
  const parts = [settings.hotkeys.dictation, settings.hotkeys.command_mode];
  const hasModifier = (s: string) =>
    /ctrl|shift|alt|win|meta|super/i.test(s) && s.includes("+");
  const hotkeys =
    parts.every((p) => p.trim().length > 0 && hasModifier(p)) &&
    new Set(parts.map((p) => p.toLowerCase())).size === 2;

  let detail = "Ready to dictate — open Notepad and hold your dictation hotkey.";
  if (!el) detail = "Add at least one ElevenLabs API key, then Save & apply.";
  else if (!groq)
    detail = "Add a Groq API key, or set cleanup to None, then Save & apply.";
  else if (!hotkeys)
    detail = "Set two different hotkeys, each with a modifier (Ctrl/Shift/Alt/Win).";

  return {
    ready: el && groq && mic && hotkeys,
    el,
    groq,
    mic,
    hotkeys,
    detail,
  };
}

function updateReadyChecklist() {
  const r = computeReady(currentSettings ?? savedSettings);
  const banner = document.getElementById("setup-ready-banner");
  const title = document.getElementById("setup-ready-title");
  const detail = document.getElementById("setup-ready-detail");
  if (title) title.textContent = r.ready ? "Ready to dictate" : "Needs setup";
  if (detail) detail.textContent = r.detail;
  if (banner) {
    banner.style.background = r.ready
      ? "rgba(109,213,140,0.10)"
      : "rgba(242,184,181,0.10)";
    banner.style.borderColor = r.ready
      ? "rgba(109,213,140,0.28)"
      : "rgba(242,184,181,0.28)";
  }
  const mark = (id: string, ok: boolean, label: string) => {
    const el = document.getElementById(id);
    if (el) el.textContent = `${ok ? "✓" : "○"} ${label}`;
  };
  mark("setup-check-el", r.el, "ElevenLabs API key");
  mark("setup-check-groq", r.groq, "Groq API key (or cleanup None)");
  mark("setup-check-mic", r.mic, "Microphone");
  mark("setup-check-hotkeys", r.hotkeys, "Hotkeys valid");

  const eng = document.getElementById("setup-engine-state");
  if (eng) eng.textContent = lastEngineState;
  const err = document.getElementById("setup-last-error");
  if (err) err.textContent = lastEngineError;
  const lastDict = document.getElementById("setup-last-dictation");
  if (lastDict) lastDict.textContent = lastDictationSummary;

  // Sidebar status when not ready
  const badge = document.getElementById("engine-status-badge");
  if (badge && lastEngineState === "Idle" && !r.ready) {
    const text = badge.querySelector(".status-text");
    if (text) text.textContent = "Needs setup";
  }
}

async function saveAndApplySettings(fromSetup: boolean) {
  if (!currentSettings) currentSettings = gatherFormSettings();
  else currentSettings = gatherFormSettings();
  if (!currentSettings) return;
  try {
    await invoke("save_settings", { settings: currentSettings });
    try {
      await invoke("set_autostart", {
        enabled: currentSettings.general.launch_at_startup,
      });
    } catch (autoErr) {
      showToast(`Settings saved, but autostart registry failed: ${autoErr}`, "error");
      savedSettings = JSON.parse(JSON.stringify(currentSettings));
      updateSaveBar();
      return;
    }
    savedSettings = JSON.parse(JSON.stringify(currentSettings));
    updateSaveBar();
    showToast(
      fromSetup ? "Saved & applied. You can dictate now if Ready is green." : "Settings saved successfully.",
      "success",
    );
  } catch (err) {
    showToast(`Failed to save settings: ${err}`, "error");
  }
}

async function refreshVaultUi() {
  try {
    const st = await invoke<{
      mode: string;
      needs_unlock: boolean;
      session_active: boolean;
      has_sealed: boolean;
    }>("vault_status");
    const modeLabel = document.getElementById("vault-mode-label");
    const detail = document.getElementById("vault-status-detail");
    if (modeLabel) {
      modeLabel.textContent =
        st.mode === "passphrase"
          ? "Passphrase vault (portable)"
          : "DPAPI (this Windows user)";
    }
    if (detail) {
      if (st.needs_unlock) detail.textContent = "Locked — unlock to use dictation keys.";
      else if (st.mode === "passphrase" && st.session_active)
        detail.textContent = "Unlocked for this session.";
      else if (st.mode === "dpapi")
        detail.textContent = "Keys encrypted for your Windows account.";
      else detail.textContent = "";
    }
    const modal = document.getElementById("vault-unlock-modal");
    if (modal) {
      modal.style.display = st.needs_unlock ? "flex" : "none";
    }
  } catch {
    /* optional */
  }
}

function setupVaultHandlers() {
  document.getElementById("vault-enable-passphrase-btn")?.addEventListener("click", async () => {
    const pass = (document.getElementById("vault-passphrase") as HTMLInputElement)?.value || "";
    try {
      await invoke("vault_enable_passphrase", { passphrase: pass });
      await loadSettings();
      await refreshVaultUi();
      showToast("Passphrase vault enabled. Save settings after key changes while unlocked.", "success");
    } catch (err) {
      showToast(`Vault error: ${err}`, "error");
    }
  });
  document.getElementById("vault-enable-dpapi-btn")?.addEventListener("click", async () => {
    try {
      await invoke("vault_enable_dpapi");
      await loadSettings();
      await refreshVaultUi();
      showToast("Switched to DPAPI (this Windows user).", "success");
    } catch (err) {
      showToast(`Vault error: ${err}`, "error");
    }
  });
  const doUnlock = async () => {
    const pass =
      (document.getElementById("vault-unlock-input") as HTMLInputElement)?.value ||
      (document.getElementById("vault-passphrase") as HTMLInputElement)?.value ||
      "";
    const errEl = document.getElementById("vault-unlock-error");
    try {
      await invoke("vault_unlock", { passphrase: pass });
      await loadSettings();
      await refreshVaultUi();
      showToast("Vault unlocked.", "success");
      if (errEl) errEl.textContent = "";
    } catch (err) {
      if (errEl) errEl.textContent = String(err);
      showToast(`Unlock failed: ${err}`, "error");
    }
  };
  document.getElementById("vault-unlock-btn")?.addEventListener("click", () => {
    void doUnlock();
  });
  document.getElementById("vault-unlock-submit")?.addEventListener("click", () => {
    void doUnlock();
  });
  document.getElementById("vault-unlock-input")?.addEventListener("keydown", (e) => {
    if ((e as KeyboardEvent).key === "Enter") void doUnlock();
  });
}

function setupSetupTabHandlers() {
  document.getElementById("setup-save-apply-btn")?.addEventListener("click", () => {
    currentSettings = gatherFormSettings();
    void saveAndApplySettings(true);
  });

  document.querySelectorAll("[data-goto]").forEach((btn) => {
    btn.addEventListener("click", () => {
      const tab = btn.getAttribute("data-goto");
      if (!tab) return;
      const item = document.querySelector(`.nav-item[data-tab="${tab}"]`) as HTMLElement | null;
      item?.click();
    });
  });

  document.getElementById("groq-key-toggle")?.addEventListener("click", () => {
    const input = document.getElementById("groq-key") as HTMLInputElement | null;
    const btn = document.getElementById("groq-key-toggle");
    if (!input || !btn) return;
    if (input.type === "password") {
      input.type = "text";
      btn.textContent = "Hide";
    } else {
      input.type = "password";
      btn.textContent = "Show";
    }
  });

  document.getElementById("setup-test-mic-btn")?.addEventListener("click", async () => {
    const btn = document.getElementById("setup-test-mic-btn") as HTMLButtonElement | null;
    const fill = document.getElementById("setup-mic-meter-fill");
    const label = document.getElementById("setup-mic-meter-label");
    const deviceSelect = document.getElementById("dict-audio-device") as HTMLSelectElement | null;
    let device = deviceSelect?.value || "system_default";
    if (device === "System default") device = "system_default";
    if (btn) {
      btn.disabled = true;
      btn.textContent = "Listening…";
    }
    if (label) label.textContent = "Speak now…";
    try {
      const level = await invoke<number>("sample_mic_level", { device });
      const pct = Math.round(Math.min(1, Math.max(0, level)) * 100);
      if (fill) (fill as HTMLElement).style.width = `${pct}%`;
      if (label) {
        label.textContent =
          pct < 5
            ? "Very quiet — check the mic or speak louder."
            : pct < 25
              ? `Level OK (${pct}%).`
              : `Strong signal (${pct}%).`;
      }
    } catch (err) {
      showToast(`Mic test failed: ${err}`, "error");
      if (label) label.textContent = "Mic test failed.";
    } finally {
      if (btn) {
        btn.disabled = false;
        btn.textContent = "Test microphone";
      }
    }
  });
}

// --- AUDIO HARDWARE ENUMERATION ---

async function loadAudioDevices() {
  try {
    const devices = await invoke<string[]>("list_input_devices");
    const deviceSelect = document.getElementById("dict-audio-device") as HTMLSelectElement;
    
    // Clear dynamic devices, keeping System default
    while (deviceSelect.options.length > 1) {
      deviceSelect.remove(1);
    }

    devices.forEach(device => {
      if (device !== "System default" && device !== "system_default") {
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
  const hkInputs = ["hk-dictation", "hk-command"];
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
        const key = id === "hk-dictation" ? "dictation" : "command_mode";
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

      // Never allow a bare key — would swallow every press system-wide.
      if (parts.length === 0) {
        showToast("Hotkeys must include at least one modifier (Ctrl, Shift, Alt, or Win).", "error");
        return;
      }

      let key = e.key.toUpperCase();
      if (key === " ") key = "SPACE";
      
      // Map arrow keys and special keys
      if (key === "ARROWUP") key = "UP";
      if (key === "ARROWDOWN") key = "DOWN";
      if (key === "ARROWLEFT") key = "LEFT";
      if (key === "ARROWRIGHT") key = "RIGHT";

      const isAlphaNum = /^[A-Z0-9]$/.test(key);
      const isAllowedSpecial = ["ENTER", "SPACE", "TAB", "ESCAPE"].includes(key);

      if (!isAlphaNum && !isAllowedSpecial) {
        showToast("Only alphanumeric keys (A-Z, 0-9) and Enter/Space/Tab/Esc are supported for hotkeys.", "error");
        input.blur();
        return;
      }

      parts.push(key);
      const combo = parts.join("+");

      // Reject duplicates against the other hotkey field.
      const others = hkInputs
        .filter((oid) => oid !== id)
        .map((oid) => (document.getElementById(oid) as HTMLInputElement | null)?.value || "");
      if (others.some((o) => o.toLowerCase() === combo.toLowerCase())) {
        showToast("That shortcut is already used by another VillFlow hotkey.", "error");
        input.blur();
        return;
      }

      input.value = combo;
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
  document.getElementById("dict-export-btn")?.addEventListener("click", async () => {
    try {
      const json = await invoke<string>("dictionary_export");
      const blob = new Blob([json], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `villflow-dictionary-${new Date().toISOString().slice(0, 10)}.json`;
      a.click();
      URL.revokeObjectURL(url);
      showToast("Dictionary exported.", "success");
    } catch (err) {
      showToast(`Export failed: ${err}`, "error");
    }
  });

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
        showToast("Word updated successfully.", "success");
      } else {
        await invoke("dictionary_add", { word, source: "manual" });
        showToast("Word added successfully.", "success");
      }
      wordModal?.classList.remove("active");
      await loadDictionary();
    } catch (err) {
      const errStr = String(err);
      if (errStr.includes("UNIQUE constraint failed")) {
        showToast(`"${word}" is already in the dictionary.`, "error");
      } else {
        showToast(`Error saving word: ${err}`, "error");
      }
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
        <td><span style="font-size:12px; padding:2px 8px; border-radius:8px; background:rgba(168,199,250,0.12); color:var(--accent); font-weight:500;">${entry.source}</span></td>
        <td>${entry.use_count}</td>
        <td>
          <span class="star-btn" style="cursor:pointer; color: ${entry.starred ? '#fdd663' : 'var(--text-disabled)'};" data-id="${entry.id}">
            ${entry.starred 
              ? `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="16" height="16" fill="#fdd663" stroke="#fdd663" stroke-width="2"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"></polygon></svg>` 
              : `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"></polygon></svg>`
            }
          </span>
        </td>
        <td>
          <div style="display:flex; gap:8px; flex-wrap:nowrap;">
            <button class="btn btn-sm edit-word" data-id="${entry.id}" data-word="${escapeHtml(entry.word)}">
              <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"></path><path d="M18.5 2.5a2.121 2.121 0 1 1 3 3L12 15l-4 1 1-4z"></path></svg>
              Edit
            </button>
            <button class="btn btn-sm btn-danger delete-word" data-id="${entry.id}">
              <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"></polyline><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path></svg>
              Delete
            </button>
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
    showToast(`Failed to load dictionary: ${err}`, "error");
  }
}

// --- ELEVENLABS KEYS ---

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
        No keys yet. Add at least one ElevenLabs key to enable dictation.
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
        <button class="btn btn-sm move-key-up" data-idx="${idx}" ${idx === 0 ? 'disabled' : ''} title="Move Up">
          <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><line x1="12" y1="19" x2="12" y2="5"></line><polyline points="5 12 12 5 19 12"></polyline></svg>
        </button>
        <button class="btn btn-sm move-key-down" data-idx="${idx}" ${idx === keys.length - 1 ? 'disabled' : ''} title="Move Down">
          <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><line x1="12" y1="5" x2="12" y2="19"></line><polyline points="19 12 12 19 5 12"></polyline></svg>
        </button>
        <button class="btn btn-sm btn-danger remove-key" data-idx="${idx}" title="Remove Key">
          <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"></polyline><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path></svg>
        </button>
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
    const textEl = groqRefreshBtn.querySelector(".btn-text");
    const iconEl = groqRefreshBtn.querySelector(".refresh-icon");
    if (textEl) textEl.textContent = "Loading...";
    if (iconEl) iconEl.classList.add("spinning");
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
      showToast("Models updated successfully.", "success");
    } catch (err) {
      showToast(`Failed to fetch models: ${err}. Ensure your Groq API key is valid.`, "error");
    } finally {
      if (textEl) textEl.textContent = "Refresh";
      if (iconEl) iconEl.classList.remove("spinning");
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
/** Per-row final text for Copy (avoids stuffing large strings into data-* attrs). */
const historyCopyText = new Map<number, string>();

async function loadHistory(append = false) {
  try {
    if (!append) {
      historyOffset = 0;
      historyCopyText.clear();
      if (historyContainer) historyContainer.innerHTML = "";
    }
    
    const list = await invoke<HistoryEntry[]>("history_list", { limit: historyLimit, offset: historyOffset });
    const loadMoreBtn = document.getElementById("history-load-more-btn") as HTMLButtonElement;
    
    if (!historyContainer) return;

    if (list.length === 0 && !append) {
      historyContainer.innerHTML = `
        <div style="text-align: center; color: var(--text-disabled); font-style: italic; padding: 40px 0;">
          No history items found.
        </div>
      `;
      if (loadMoreBtn) loadMoreBtn.style.display = "none";
      return;
    }

    list.forEach(entry => {
      // Engine stores local wall-clock ISO without timezone; parse as local, not UTC.
      const date = parseLocalIso(entry.ts);
      const timeStr = date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
      const dateStr = date.toLocaleDateString([], { month: 'short', day: 'numeric' });
      const entryId = entry.id ?? 0;
      if (entryId) {
        historyCopyText.set(entryId, entry.final_text);
      }
      
      const row = document.createElement("div");
      row.className = "history-row";
      row.innerHTML = `
        <div class="history-header">
          <div class="history-meta">
            <span>${dateStr} ${timeStr}</span>
            <span class="app-badge">${escapeHtml(entry.app_name)}</span>
            <span style="text-transform: capitalize; color: var(--accent); font-weight: 600;">${escapeHtml(entry.mode)}</span>
          </div>
          <div class="history-summary">${escapeHtml(entry.final_text)}</div>
          <span class="expand-arrow" style="display: inline-flex; align-items: center; justify-content: center;">
            <svg class="chevron-icon" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"></polyline></svg>
          </span>
        </div>
        <div class="history-body-wrapper">
          <div class="history-body-inner">
            <div class="history-body">
              <div class="transcript-block">
                <div>
                  <div class="transcript-label">RAW TRANSCRIPT</div>
                  <div class="transcript-box">${escapeHtml(entry.raw_transcript)}</div>
                </div>
                <div>
                  <div class="transcript-label">RESULT</div>
                  <div class="transcript-box">${escapeHtml(entry.final_text)}</div>
                </div>
              </div>
              <div style="display:flex; justify-content: space-between; align-items:center; font-size:12px; color: var(--text-muted); margin-top: 4px;">
                <div>
                  <span>${(entry.duration_ms / 1000).toFixed(1)}s</span>
                  <span style="margin: 0 8px; color: var(--border-color);">|</span>
                  <span>${entry.word_count} words</span>
                </div>
                <div style="display:flex; gap:8px;">
                  <button class="btn btn-sm copy-history-text" data-id="${entryId}">Copy</button>
                  <button class="btn btn-sm btn-danger delete-history" data-id="${entryId}">Delete</button>
                </div>
              </div>
            </div>
          </div>
        </div>
      `;
      historyContainer.appendChild(row);
    });

    // Wire expandable accordion logic for dynamically added rows
    const headers = historyContainer.querySelectorAll(".history-header");
    headers.forEach(header => {
      if (!(header as any).hasListener) {
        (header as any).hasListener = true;
        header.addEventListener("click", () => {
          const row = header.closest(".history-row");
          if (!row) return;
          const isExpanded = row.classList.contains("expanded");
          
          historyContainer.querySelectorAll(".history-row").forEach(r => r.classList.remove("expanded"));

          if (!isExpanded) {
            row.classList.add("expanded");
          }
        });
      }
    });

    // Wire copy buttons safely
    const copyBtns = historyContainer.querySelectorAll(".copy-history-text");
    copyBtns.forEach(btn => {
      if (!(btn as any).hasListener) {
        (btn as any).hasListener = true;
        btn.addEventListener("click", async (e) => {
          e.stopPropagation();
          const id = parseInt(btn.getAttribute("data-id") || "0");
          const text = historyCopyText.get(id) || "";
          try {
            await navigator.clipboard.writeText(text);
            const origText = btn.innerHTML;
            btn.innerHTML = "Copied!";
            setTimeout(() => { btn.innerHTML = origText; }, 1500);
          } catch (err) {
            console.error("Failed to copy text:", err);
            showToast("Failed to copy to clipboard.", "error");
          }
        });
      }
    });

    // Wire per-row delete buttons
    const deleteBtns = historyContainer.querySelectorAll(".delete-history");
    deleteBtns.forEach(btn => {
      if (!(btn as any).hasListener) {
        (btn as any).hasListener = true;
        btn.addEventListener("click", async (e) => {
          e.stopPropagation();
          const id = parseInt(btn.getAttribute("data-id") || "0");
          if (!id) return;
          if (!confirm("Delete this history entry?")) return;
          try {
            await invoke("history_delete", { id });
            historyCopyText.delete(id);
            btn.closest(".history-row")?.remove();
            if (historyContainer.querySelectorAll(".history-row").length === 0) {
              await loadHistory();
            }
          } catch (err) {
            showToast(`Failed to delete entry: ${err}`, "error");
          }
        });
      }
    });

    // Handle "Load More" visibility
    if (loadMoreBtn) {
      if (list.length === historyLimit) {
        loadMoreBtn.style.display = "inline-flex";
      } else {
        loadMoreBtn.style.display = "none";
      }
    }

  } catch (err) {
    console.error("Failed to load history list:", err);
    showToast(`Failed to load history: ${err}`, "error");
  }
}

// --- INSIGHTS ---

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
    showToast(`Failed to load insights: ${err}`, "error");
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
  
  // Prepend empty cells to align with the starting day of the week (local).
  const startDate = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 365);
  const startDay = startDate.getDay();
  for (let s = 0; s < startDay; s++) {
    const cell = document.createElement("div");
    cell.className = "heatmap-cell";
    cell.style.opacity = "0";
    cell.style.pointerEvents = "none";
    grid.appendChild(cell);
  }

  // Generate boxes starting from 365 days ago up to today (local calendar dates).
  for (let i = 365; i >= 0; i--) {
    const dayDate = new Date(now.getFullYear(), now.getMonth(), now.getDate() - i);
    const dayStr = localDateStr(dayDate);
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
    grid.appendChild(cell);
  }
}

// --- UTILITIES ---

/** Local calendar date as YYYY-MM-DD (matches store local ts day keys). */
function localDateStr(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/**
 * Parse `YYYY-MM-DDTHH:MM:SS` (or date-only) as a local Date.
 * Avoid `new Date(iso)` which some engines treat as UTC when no offset is present.
 */
function parseLocalIso(ts: string): Date {
  const m = /^(\d{4})-(\d{2})-(\d{2})(?:[T ](\d{2}):(\d{2})(?::(\d{2}))?)?/.exec(ts.trim());
  if (!m) {
    const fallback = new Date(ts);
    return Number.isNaN(fallback.getTime()) ? new Date() : fallback;
  }
  const year = Number(m[1]);
  const month = Number(m[2]) - 1;
  const day = Number(m[3]);
  const hour = Number(m[4] ?? "0");
  const min = Number(m[5] ?? "0");
  const sec = Number(m[6] ?? "0");
  return new Date(year, month, day, hour, min, sec);
}

function escapeHtml(unsafe: string): string {
  return unsafe
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

function showToast(message: string, type: "success" | "error" | "info" = "info") {
  const container = document.getElementById("toast-container");
  if (!container) return;

  const toast = document.createElement("div");
  toast.className = `toast toast-${type}`;
  toast.innerHTML = `
    <span class="toast-icon"></span>
    <span>${escapeHtml(message)}</span>
  `;

  container.appendChild(toast);

  // Force reflow
  setTimeout(() => toast.classList.add("active"), 10);

  // Remove toast after 4 seconds
  setTimeout(() => {
    toast.classList.remove("active");
    setTimeout(() => toast.remove(), 300);
  }, 4000);
}

async function setupEngineEventListeners() {
  const statusBadge = document.getElementById("engine-status-badge");
  const statusText = statusBadge?.querySelector(".status-text");

  if (statusBadge && statusText) {
    try {
      await listen<string>("engine-state", (event) => {
        const state = event.payload;
        lastEngineState = state;
        // Reset state classes
        statusBadge.className = "engine-status";
        const r = computeReady(currentSettings ?? savedSettings);
        if (state === "Idle" && !r.ready) {
          statusText.textContent = "Needs setup";
        } else {
          statusText.textContent = state;
        }

        if (state === "Connecting") {
          statusBadge.classList.add("connecting");
        } else if (state === "Recording") {
          statusBadge.classList.add("recording");
        } else if (state === "Processing") {
          statusBadge.classList.add("processing");
        } else if (state === "Injecting") {
          statusBadge.classList.add("injecting");
        }
        updateReadyChecklist();
      });

      await listen<string>("engine-error", (event) => {
        const errMsg = event.payload;
        lastEngineError = errMsg || "(none)";
        updateReadyChecklist();
        showToast(`Engine Error: ${errMsg}`, "error");
      });

      await listen<{ words: number; total_ms: number }>("engine-injected", (event) => {
        const w = event.payload?.words ?? 0;
        const ms = event.payload?.total_ms ?? 0;
        lastDictationSummary = `${w} word${w === 1 ? "" : "s"} in ${ms} ms`;
        updateReadyChecklist();
      });

      // Dictation into Settings fields only when *this* window is focused.
      await listen<string>("app-insert", async (event) => {
        try {
          const { getCurrentWindow } = await import("@tauri-apps/api/window");
          if (!(await getCurrentWindow().isFocused())) return;
        } catch {
          if (!document.hasFocus()) return;
        }
        const text = event.payload ?? "";
        if (!text) return;
        const el = document.activeElement as HTMLElement | null;
        if (!el) return;
        if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
          const start = el.selectionStart ?? el.value.length;
          const end = el.selectionEnd ?? el.value.length;
          const before = el.value.slice(0, start);
          const after = el.value.slice(end);
          el.value = before + text + after;
          const caret = start + text.length;
          el.selectionStart = caret;
          el.selectionEnd = caret;
          el.dispatchEvent(new Event("input", { bubbles: true }));
        } else if (el.isContentEditable) {
          document.execCommand("insertText", false, text);
        }
      });
    } catch (err) {
      console.error("Failed to setup engine state event listeners:", err);
    }
  }
}

// --- BOOTSTRAP ---

window.addEventListener("DOMContentLoaded", () => {
  init();
});
