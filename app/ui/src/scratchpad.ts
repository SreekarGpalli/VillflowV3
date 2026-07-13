import { invoke } from "@tauri-apps/api/core";
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
          // Unwrap disallowed tags (keep text children).
          while (el.firstChild) {
            node.insertBefore(el.firstChild, el);
          }
          node.removeChild(el);
          continue;
        }
        // Strip all attributes (no href/on*/style XSS vectors).
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

async function init() {
  editor = document.getElementById("scratchpad-editor") as HTMLDivElement;
  saveStatus = document.getElementById("save-status");
  wordCountDisplay = document.getElementById("word-count");

  if (!editor) return;

  // Initialize toolbar buttons
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

  // Load initial content
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

  // Intercept close button to hide instead of destroying
  try {
    const appWindow = getCurrentWindow();
    await appWindow.onCloseRequested(async (event) => {
      event.preventDefault();
      await appWindow.hide();
    });
  } catch (e) {
    console.error("Failed to setup close handler:", e);
  }
}

window.addEventListener("DOMContentLoaded", init);
