/**
 * Scratchpad — plain-text editor (reliable dictation insert).
 * Toolbar inserts Markdown-style markers; content is stored as text/HTML-escaped plain.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

let editor: HTMLTextAreaElement | null = null;
let saveStatus: HTMLElement | null = null;
let wordCountDisplay: HTMLElement | null = null;
let debounceTimeout: number | null = null;

/** Strip HTML from legacy rich-content stores. */
function htmlToPlain(html: string): string {
  if (!html.includes("<")) return html;
  const tmp = document.createElement("div");
  tmp.innerHTML = html;
  return (tmp.innerText || tmp.textContent || "").replace(/\u00a0/g, " ");
}

function updateWordCount(text: string) {
  if (!wordCountDisplay) return;
  const clean = text.trim();
  if (!clean) {
    wordCountDisplay.innerText = "0 words";
    return;
  }
  const words = clean.split(/\s+/).length;
  wordCountDisplay.innerText = `${words} word${words === 1 ? "" : "s"}`;
}

async function saveContent() {
  if (!editor || !saveStatus) return;
  saveStatus.innerText = "Saving…";
  saveStatus.className = "status-saving";
  const content = editor.value;
  try {
    await invoke("scratchpad_set", { content });
    saveStatus.innerText = "Saved";
    saveStatus.className = "status-saved";
  } catch (err) {
    saveStatus.innerText = `Error: ${err}`;
    saveStatus.className = "status-error";
  }
}

function triggerSave() {
  if (saveStatus) {
    saveStatus.innerText = "Unsaved changes";
    saveStatus.className = "status-unsaved";
  }
  if (debounceTimeout) clearTimeout(debounceTimeout);
  debounceTimeout = window.setTimeout(saveContent, 400);
}

function wrapSelection(before: string, after: string) {
  if (!editor) return;
  const start = editor.selectionStart;
  const end = editor.selectionEnd;
  const value = editor.value;
  const selected = value.slice(start, end) || "text";
  const next = value.slice(0, start) + before + selected + after + value.slice(end);
  editor.value = next;
  const caret = start + before.length + selected.length + after.length;
  editor.selectionStart = editor.selectionEnd = caret;
  editor.focus();
  updateWordCount(editor.value);
  triggerSave();
}

function prefixLine(prefix: string) {
  if (!editor) return;
  const start = editor.selectionStart;
  const value = editor.value;
  const lineStart = value.lastIndexOf("\n", start - 1) + 1;
  editor.value = value.slice(0, lineStart) + prefix + value.slice(lineStart);
  editor.selectionStart = editor.selectionEnd = start + prefix.length;
  editor.focus();
  updateWordCount(editor.value);
  triggerSave();
}

function insertDictatedText(text: string) {
  if (!editor || !text) return;
  editor.focus();
  const start = editor.selectionStart;
  const end = editor.selectionEnd;
  const value = editor.value;
  const needsSpace =
    start > 0 &&
    !/\s$/.test(value.slice(0, start)) &&
    !/^\s/.test(text);
  const insert = (needsSpace ? " " : "") + text;
  editor.value = value.slice(0, start) + insert + value.slice(end);
  const caret = start + insert.length;
  editor.selectionStart = editor.selectionEnd = caret;
  updateWordCount(editor.value);
  triggerSave();
}

function normalizeState(payload: unknown): string {
  if (typeof payload === "string") return payload;
  if (payload && typeof payload === "object") {
    const keys = Object.keys(payload as object);
    if (keys.length === 1) return keys[0];
  }
  return String(payload ?? "");
}

function setDictationPill(stateRaw: unknown) {
  const pill = document.getElementById("dictation-pill");
  const label = document.getElementById("dictation-pill-label");
  if (!pill || !label) return;
  const state = normalizeState(stateRaw);
  pill.classList.remove("visible", "recording", "processing", "injecting");
  if (state === "Recording" || state === "Processing" || state === "Injecting") {
    pill.classList.add("visible", state.toLowerCase());
    label.textContent = state;
  }
}

async function init() {
  editor = document.getElementById("scratchpad-editor") as HTMLTextAreaElement;
  saveStatus = document.getElementById("save-status");
  wordCountDisplay = document.getElementById("word-count");
  if (!editor) return;

  document.getElementById("btn-bold")?.addEventListener("click", () => wrapSelection("**", "**"));
  document.getElementById("btn-italic")?.addEventListener("click", () => wrapSelection("_", "_"));
  document.getElementById("btn-bullets")?.addEventListener("click", () => prefixLine("- "));
  document.getElementById("btn-numbers")?.addEventListener("click", () => prefixLine("1. "));
  document.getElementById("btn-undo")?.addEventListener("click", () => {
    document.execCommand("undo");
    updateWordCount(editor?.value || "");
    triggerSave();
  });
  document.getElementById("btn-clear")?.addEventListener("click", () => {
    if (!editor) return;
    if (!confirm("Clear the entire scratchpad?")) return;
    editor.value = "";
    updateWordCount("");
    triggerSave();
    editor.focus();
  });

  try {
    const initial = await invoke<string>("scratchpad_get");
    editor.value = htmlToPlain(initial || "");
    updateWordCount(editor.value);
  } catch (err) {
    console.error("Failed to load scratchpad:", err);
  }

  editor.addEventListener("input", () => {
    updateWordCount(editor?.value || "");
    triggerSave();
  });

  try {
    await listen<string>("app-insert", async (event) => {
      const text = event.payload ?? "";
      if (!text) return;
      try {
        if (!(await getCurrentWindow().isVisible())) return;
      } catch {
        /* continue */
      }
      insertDictatedText(text);
    });
    await listen("engine-state", (event) => setDictationPill(event.payload));
    await listen("scratchpad-focus", () => editor?.focus());
  } catch (e) {
    console.error("Failed to listen for app events:", e);
  }

  try {
    const appWindow = getCurrentWindow();
    await appWindow.onCloseRequested(async (event) => {
      event.preventDefault();
      await appWindow.hide();
    });
  } catch (e) {
    console.error("Failed to setup close handler:", e);
  }

  editor.focus();
}

window.addEventListener("DOMContentLoaded", init);
