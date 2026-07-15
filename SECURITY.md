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
| `%APPDATA%\VillFlow\villflow.db` | Dictionary, history, scratchpad |
| `%APPDATA%\VillFlow\logs\villflow.log` | Application logs (keys are never logged) |

API keys are held in memory as plaintext while the app runs (needed for ElevenLabs / Groq). On disk they are wrapped as `vfdpapi1:` + DPAPI ciphertext. Older plaintext keys are accepted once and re-encrypted on the next save. DPAPI secrets are bound to your Windows user profile — they will not decrypt under a different account.

**Never** commit `settings.json`, API keys, or copies of `%APPDATA%\VillFlow\` into this repository.

Network traffic is limited to:

- [ElevenLabs](https://elevenlabs.io/) realtime speech-to-text WebSocket
- [Groq](https://groq.com/) OpenAI-compatible chat completions / model list

There is no VillFlow account system, no telemetry, and no auto-update phone-home.
