import { invoke } from "@tauri-apps/api/core";

async function fetchSettings() {
  const display = document.getElementById("settings-display");
  if (!display) return;
  
  try {
    const settings = await invoke("get_settings");
    display.innerHTML = `<pre>${JSON.stringify(settings, null, 2)}</pre>`;
  } catch (error) {
    // If running in a normal browser dev environment without Tauri context, provide a clear mock indicator
    if (window.navigator.userAgent.includes("Tauri") || (window as any).__TAURI_INTERNALS__) {
      display.innerHTML = `<div style="color: #ef4444; font-weight: 500;">Error invoking get_settings: ${error}</div>`;
    } else {
      display.innerHTML = `
        <div style="color: #eab308; font-weight: 500; margin-bottom: 8px;">Running outside Tauri container. Mock settings data:</div>
        <pre>${JSON.stringify({
          version: 1,
          general: { launch_at_startup: false, start_minimized: false, show_error_notifications: true },
          hotkeys: { dictation: "Ctrl+Shift+Z", command_mode: "Ctrl+Shift+X", scratchpad: "Ctrl+Shift+C" },
          audio: { input_device: "system_default" },
          stt: { api_keys: [], endpoint: "wss://api.elevenlabs.io", model_id: "scribe_v2_realtime", language_code: "en" },
          llm: { api_key: "", model: "openai/gpt-oss-120b", cleanup_level: "medium" },
          output: { injection_method: "clipboard_paste", restore_clipboard: true },
          dictionary: { auto_learn: true }
        }, null, 2)}</pre>
      `;
    }
  }
}

window.addEventListener("DOMContentLoaded", () => {
  fetchSettings();
});
