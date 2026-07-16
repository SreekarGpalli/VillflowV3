# Security Policy

## Supported versions

| Version | Supported |
| ------- | --------- |
| 0.1.x   | Yes       |

## Reporting a vulnerability

Please **do not** open a public GitHub issue for security problems.

Email or privately message the maintainer with:

- A clear description of the issue
- Steps to reproduce (if possible)
- Impact assessment (e.g. key leakage, arbitrary code, local privilege)

We will acknowledge reports as soon as practical and work on a fix before any public disclosure.

## API keys and local data

VillFlow stores settings and API keys **only on your machine**:

| Path | Contents |
| ---- | -------- |
| `%APPDATA%\VillFlow\settings.json` | Settings; API keys encrypted at rest with **Windows DPAPI** (current user) |
| `%APPDATA%\VillFlow\villflow.db` | Dictionary, history |
| `%APPDATA%\VillFlow\logs\villflow.log` | Application logs (keys are never logged) |

API keys are held in memory as plaintext while the app runs (needed for ElevenLabs / Groq).

**At rest (choose one vault mode in General):**

| Mode | Behavior |
|------|----------|
| **DPAPI** (default) | Per-key `vfdpapi1:` + Windows DPAPI. Bound to your Windows user profile. |
| **Passphrase** | AES-256-GCM sealed blob (PBKDF2-HMAC-SHA256). Portable if you remember the passphrase — use when moving settings to another PC. |

Legacy plaintext keys are accepted once and re-protected on the next save.

**Never** commit `settings.json`, API keys, or copies of `%APPDATA%\VillFlow\` into this repository.

Network traffic is limited to:

- [ElevenLabs](https://elevenlabs.io/) realtime speech-to-text WebSocket
- [Groq](https://groq.com/) OpenAI-compatible chat completions / model list

There is no VillFlow account system, no telemetry, and no auto-update phone-home.
