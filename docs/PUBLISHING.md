# Publishing VillFlow to GitHub

Step-by-step checklist for the first public release.

**Artifacts (PRODUCT.md):** ship **both** a portable `villflow.exe` and a Windows installer (NSIS and/or MSI via Tauri bundle). Document both on the Release page.

## 1. Create the GitHub repository

1. On GitHub: **New repository** → name `VillFlow` (or your choice)
2. **Do not** initialize with a README (this repo already has one)
3. Visibility: Public or Private as you prefer

## 2. Point `origin` and push

```powershell
cd path\to\VillFlow
git remote add origin https://github.com/YOUR_USER/VillFlow.git
git branch -M main
git push -u origin main
```

If you stay on `master`, CI still runs (both branch names are configured).

## 3. Fix the README badge

README already points at `https://github.com/SreekarGpalli/VillflowV3`. If you rename the repo or org, update the CI badge and clone URLs in `README.md`.

## 4. Cut the v0.1.0 release

```powershell
git tag -a v0.1.0 -m "VillFlow v0.1.0 — first public release"
git push origin v0.1.0
```

The **Release** workflow builds `villflow.exe` on `windows-latest` and creates a GitHub Release with the binary attached.

Watch: **Actions → Release**.

## 5. Manual release (if you prefer local build)

```powershell
cd app\ui
npm ci
cd ..
ui\node_modules\.bin\tauri.cmd build --no-bundle
# Binary: ..\target\release\villflow.exe
```

Then on GitHub: **Releases → Draft a new release** → tag `v0.1.0` → upload `villflow.exe` → paste notes from `CHANGELOG.md`.

## 6. Post-release sanity

- [ ] Download the exe from the Release page on a clean Windows machine
- [ ] Add ElevenLabs + Groq keys under **AI Services**
- [ ] Dictation (`Ctrl+Shift+Z`) into Notepad
- [ ] Command mode (`Ctrl+Shift+X`) with and without selection
- [ ] Confirm log at `%APPDATA%\VillFlow\logs\villflow.log`

## What is not published

These stay only on your machine under `docs/internal/` (gitignored):

- Original product notes
- Multi-agent verification reports
- Third-party reference material

Do **not** commit API keys or copies of `%APPDATA%\VillFlow\`.
