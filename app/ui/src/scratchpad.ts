import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

let editor: HTMLDivElement | null = null;
let saveStatus: HTMLSpanElement | null = null;
let wordCountDisplay: HTMLSpanElement | null = null;
let debounceTimeout: number | null = null;

/** Allowlisted tags for scratchpad rich text (bold/italic/lists). */
const ALLOWED_TAGS = new Set([
  "B", "I", "EM", "STRONG", "U", "UL", "OL", "LI", "P", "BR", "DIV", "SPAN",
]);

function sanitizeHtml(html: string): string {
  const temp = document.createElement("div");
  temp.innerHTML = html;

  const walk = (node: Node) => {
    const children = Array.from(node.childNodes);
    for (const child of children) {
      if (child.nodeType === Node.ELEMENT_NODE) {
        const el = child as HTMLElement;
        if (!ALLOWED_TAGS.has(el.tagName)) {
          while (el.firstChild) {
            node.insertBefore(el.firstChild, el);
          }
          node.removeChild(el);
          continue;
        }
        for (const attr of Array.from(el.attributes)) {
          el.removeAttribute(attr.name);
        }
        walk(el);
      }
    }
  };
  walk(temp);
  return temp.innerHTML;
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
  saveStatus.innerText = "Saving...";
  saveStatus.className = "status-saving";

  const content = sanitizeHtml(editor.innerHTML);
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
  if (debounceTimeout) {
    clearTimeout(debounceTimeout);
  }
  debounceTimeout = window.setTimeout(saveContent, 500);
}

function normalizeState(payload: unknown): string {
  if (typeof payload === "string") return payload;
  if (payload && typeof payload === "object") {
    // Defensive: if a tagged object ever arrives.
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

/**
 * Insert dictated / command-mode text at the caret (or replace the selection).
 */
function insertDictatedText(text: string) {
  if (!editor || !text) return;

  try {
    editor.focus();
  } catch {
    /* ignore */
  }

  let inserted = false;
  try {
    inserted = document.execCommand("insertText", false, text);
  } catch {
    inserted = false;
  }

  if (!inserted) {
    const sel = window.getSelection();
    if (sel && sel.rangeCount > 0) {
      const range = sel.getRangeAt(0);
      if (editor.contains(range.commonAncestorContainer) || range.commonAncestorContainer === editor) {
        range.deleteContents();
        const node = document.createTextNode(text);
        range.insertNode(node);
        range.setStartAfter(node);
        range.collapse(true);
        sel.removeAllRanges();
        sel.addRange(range);
        inserted = true;
      }
    }
  }

  if (!inserted) {
    // Absolute fallback: append at end of editor.
    if (editor.lastChild && editor.lastChild.nodeType === Node.TEXT_NODE) {
      editor.lastChild.textContent = (editor.lastChild.textContent || "") + text;
    } else {
      editor.appendChild(document.createTextNode(text));
    }
  }

  updateWordCount(editor.innerText || "");
  triggerSave();
}

async function init() {
  editor = document.getElementById("scratchpad-editor") as HTMLDivElement;
  saveStatus = document.getElementById("save-status");
  wordCountDisplay = document.getElementById("word-count");

  if (!editor) return;

  document.getElementById("btn-bold")?.addEventListener("click", () => {
    document.execCommand("bold", false);
    editor?.focus();
    triggerSave();
  });

  document.getElementById("btn-italic")?.addEventListener("click", () => {
    document.execCommand("italic", false);
    editor?.focus();
    triggerSave();
  });

  document.getElementById("btn-bullets")?.addEventListener("click", () => {
    document.execCommand("insertUnorderedList", false);
    editor?.focus();
    triggerSave();
  });

  document.getElementById("btn-numbers")?.addEventListener("click", () => {
    document.execCommand("insertOrderedList", false);
    editor?.focus();
    triggerSave();
  });

  document.getElementById("btn-undo")?.addEventListener("click", () => {
    document.execCommand("undo", false);
    editor?.focus();
    updateWordCount(editor?.innerText || "");
    triggerSave();
  });

  document.getElementById("btn-clear")?.addEventListener("click", () => {
    if (!editor) return;
    if (!confirm("Clear the entire scratchpad?")) return;
    editor.innerHTML = "";
    updateWordCount("");
    triggerSave();
    editor.focus();
  });

  try {
    const initialContent = await invoke<string>("scratchpad_get");
    editor.innerHTML = sanitizeHtml(initialContent);
    updateWordCount(editor.innerText);
  } catch (err) {
    console.error("Failed to load scratchpad:", err);
  }

  editor.addEventListener("input", () => {
    updateWordCount(editor?.innerText || "");
    triggerSave();
  });

  // --- Engine events ---
  // The shell only emits app-insert to the target window (focused / open).
  // Still guard: if Scratchpad is hidden, never apply inserts.
  try {
    await listen<string>("app-insert", async (event) => {
      const text = event.payload ?? "";
      if (!text) return;
      try {
        const appWindow = getCurrentWindow();
        if (!(await appWindow.isVisible())) return;
      } catch {
        /* proceed if visibility check fails */
      }
      console.info("[scratchpad] app-insert", text.length, "chars");
      insertDictatedText(text);
    });

    await listen("engine-state", (event) => {
      setDictationPill(event.payload);
    });

    await listen("scratchpad-focus", () => {
      editor?.focus();
    });
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
