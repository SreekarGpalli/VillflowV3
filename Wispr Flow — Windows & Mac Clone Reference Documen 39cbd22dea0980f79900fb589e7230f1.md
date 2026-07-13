# Wispr Flow — Windows & Mac Clone Reference Document

*Focused build reference for a personal desktop-only application*

---

## 1. What Is It?

An **AI-powered voice-to-text dictation app** that sits as a system-level overlay. You speak naturally — including filler words, rambling, grammar mistakes — and it instantly outputs clean, polished, properly punctuated text into whatever app or text field is active on your screen.

**Core metric:** 4× faster than typing (220 wpm speaking vs. 45 wpm typing)

---

## 2. How the Pipeline Works

Two AI stages run sequentially in the cloud:

**Stage 1 — ASR (Automatic Speech Recognition)**

- Receives your raw audio
- Reads the surrounding text in your active text field *before* transcribing (context-conditioned)
- Target: **<200ms**

**Stage 2 — LLM Post-Processing**

- Takes the raw transcript and formats/edits it
- Removes fillers, fixes grammar, applies punctuation, formats lists
- Target: **<200ms**

**Total end-to-end target: <700ms** from when you stop speaking to polished text appearing in your app.

**Context Awareness:** Flow also reads the app name you're in and optionally reads broader screen content to improve accuracy.

---

## 3. Complete Desktop Feature Set

### Core Transcription

| Feature | What It Does | Implementation  | What I need |
| --- | --- | --- | --- |
| Universal text field injection | Pastes result directly into any focused text field in any app | Yes | Same as Intended |
| Filler word removal | Strips "uh", "um", "like", "you know" automatically | Yes | Same as Intended |
| Auto punctuation | Adds periods, commas, question marks intelligently | Yes | Same as Intended |
| Grammar correction | Fixes their/they're, run-on sentences, capitalization | Yes | Same as Intended |
| List formatting | Converts spoken lists into bulleted/numbered lists | Yes | Same as Intended |
| Sentence splitting | Breaks run-ons into clean separate sentences | Yes | Same as Intended |
| Context continuation | Reads existing text in the field to continue naturally | Yes | Accepted if any Limitations |
| Smart name spelling | Uses context to correctly spell uncommon proper nouns | Yes | Accepted if any Limitations |
| 100+ language support | Auto-detects language; supports Spanish, Hindi, Chinese, Arabic, etc. | No | English Only - ( Indian Accent) |
| Whisper mode | Works when speaking very quietly | No | Too Complex |

### Editing & Commands

| Feature | What It Does | Implementation  | What I need |
| --- | --- | --- | --- |
| Auto Cleanup levels | Choose: None / Light / Medium / High — controls aggressiveness of AI editing | Yes | in Settings |
| Command Mode | Speak commands like "make this more formal" or "fix the last paragraph" | Yes | I want a separate Hot key for it. It should not collide with STT.  (Ctrl+Shift+Z for STT, Ctrl+Shift+X for Command Mode) |
| Backtrack | Say "backtrack" to remove what you just said | No  | Too Complex |
| Transforms (Beta) | Highlight any text → press shortcut → AI rewrites it (Polish, Prompt Engineer, or custom presets) | No | Too Complex |
| Undo AI edit | In history, revert to raw transcript any time | No  | Too Complex |

### Personalization

| Feature | What It Does | Implementation  | What I need |
| --- | --- | --- | --- |
| Personal Dictionary | Learns your words as you correct them; auto-adds; supports manual entries | Yes | Accepted if any Limitations |
| Starred Dictionary Words | Pin critical terms; Flow prioritizes them during transcription | Yes | Accepted if any Limitations |
| Snippet Library | Voice shortcuts: say "Calendar" → inserts full Calendly link or any text you define | No | Too Complex |
| Styles | Set writing tone globally or per-app (formal, casual, etc.) | No | Too Complex  |

### Desktop-Specific UX

| Feature | What It Does | Implementation  | What I need |
| --- | --- | --- | --- |
| Flow Bar | System overlay bar with trigger, language picker, style indicator | Yes | Simple nothing complex, Instead of big animations. simple animations and Text conformation like Recording, Processing |
| Keyboard shortcut triggers | Push-to-talk or push-on/push-off; fully customizable keys | Yes (only Push-to-talk no need for push-on/push-off) | “Ctrl+Shift+Z” for STT, “Ctrl+Shift+X” for Command Mode as Defaults |
| Mouse Flow | Bind any non-primary mouse button to start/stop dictation | No | Too Complex |
| Microphone auto-ranking | Rank mics; Flow auto-switches if active mic is unplugged | No | Not my Requirement |
| Clamshell mode | When laptop lid is closed, auto-switches to external mic | Yes, It should default to system default. No need to proactively setup anything | Show options in settings |
| 20-minute sessions | Single dictation session up to 20 minutes; 19-min warning | No | Sessions won't be that long, so no need to worry about it.  |
| Scratchpad | Floating notepad (Option+S on Mac); rich text, tabs, version history | Yes | Ctrl+Shift+C for Windows |
| Session recovery | If you quit mid-dictation, audio is saved; recover from History on reopen | No | Too Complex |
| Status bar integration | Shows dictation state at a glance | Yes |  |
| App/website localization | UI available in English, German, Spanish, Italian, Portuguese | No | Not my requirement.  |
| Insights Tab | Words per minute, total words dictated, top apps used, streak heatmap, communication profile | Yes | Accepted if any Limitations |

---

## 4. Technical Architecture to Replicate

### System-Level Components Required

**1. Audio Capture Layer**

- Captures microphone input system-wide
- Push-to-talk or toggle trigger via global keyboard
- Microphone selection in settings
- Must work while any other app is in focus

**2. System Overlay / UI Layer**

- Always-on-top floating bar (the "Flow Bar")
- Shows listening state (Simple animation)
- Global hotkey registration
- Settings

**3. ASR (Speech-to-Text) Engine**

- Cloud API call with audio payload
- Response: raw transcript
- Target: <200ms server-side

**4. LLM Formatting Engine**

- Takes raw transcript + context (app name, surrounding text, user style profile)
- Returns: cleaned, formatted text
- Target: <200ms server-side

**5. Text Injection Layer**

- Detects the currently focused text field in the active application
- Pastes output text at cursor position
- Must work across all apps (not just web — native apps too)

**6. Context Reading Layer**

- Reads existing text in the active text field before sending to LLM
- Reads app name/window title to determine correct output

**7. Personalization Store (Local)**

- Personal Dictionary (words + weights)
- Store in local database

**8. History & Scratchpad**

- Local Database or Files: store transcripts with timestamps
- Scratchpad: floating rich text editor window

**9. Settings UI (Hub)**

- Microphone settings, shortcut config, dictionary manager, style settings, Auto Cleanup level, Launch at startup, Start minimized, Show notification on error, Hotkeys settings,  The API keys andservice settings ( includes 2 APIs and their settings along with model picks), All system prompts where they can be edited and also set back to defaults, Text output settings, About and system settings.
- The order mentioned above is random. You can take everything properly and arrange them properly.

### Core Data Flow

```
[Mic Input]
  → [Audio Buffer]
  → [Cloud ASR API + context text]
  → [Raw Transcript]
  → [LLM API + app name + style profile + raw transcript]
  → [Polished Text]
  → [Text Injection into active app's focused field]
```

---

**For your personal app:** Keep everything **100% local**.

- Dictionary, Snippets, History → local SQLite
- Use Eleven Labs API and Groq API
- No account system needed

**What always happens regardless:**

- Transcription goes to the cloud (needed for accuracy and speed)
- App/window name always used for tone-matching
- Usage statistics (word counts) always tracked locally

---

## 5. Core UX Patterns to Implement

1. **Global hotkey** — Push and hold keystroke starts/stops dictation from any app, any context
2. **Simple animation** — visual feedback that the app is listening
3. **Instant paste** — result appears at cursor with no extra steps
4. **Non-blocking overlay** — UI floats above everything but does not steal focus
5. **Auto Cleanup level selector** — let user pick how much AI editing to apply
6. **Dictionary auto-learn** — watch for edits to pasted text; if user corrects a word, auto-add it ( can be edited in settings)
7. **History panel** — list of recent transcripts with copy option

---

**The single biggest differentiator to replicate:**
Context-conditioned ASR + LLM that reads your text field *before* transcribing. This is what makes it dramatically more accurate than any tool that treats each dictation as isolated audio.

```

```