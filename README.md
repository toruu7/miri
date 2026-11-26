# Desktop Cat Companion (Windows 2.5D)

A personal, low-overhead Windows 10/11 desktop companion inspired by LittleOne. Built in Rust with a pure-CPU 2.5D vector renderer (`tiny-skia`), native Win32 `UpdateLayeredWindowIndirect` compositing, zero GPU/UIA DLL imports in the resident overlay, and a separate on-demand Cat Designer (`cat-designer.exe`).

## Architecture

| Crate / Binary | Role | Dependencies |
| :--- | :--- | :--- |
| `crates/pet-core` | Pure-Rust simulation, 2.5D projection, 65% cat-facing keymap, 4-stage pacing (`60/24/12/0 FPS`), privacy state planes, 15 built-in presets, versioned schemas | `serde`, `serde_json` (zero OS/GUI deps) |
| `crates/pet-render` | Pure-Rust 8-pass 2.5D vector renderer (`tiny-skia`), static desk/keyboard sprite caching, `Mask` coat markings, contrast-adaptive dark-coat rim outlines, geometric vector keycap font | `pet-core`, `tiny-skia` (zero OS/GUI deps) |
| `crates/cat-overlay` | Resident Windows overlay (`cat-overlay.exe`): `UpdateLayeredWindowIndirect` dirty rects, `CreateWaitableTimerExW` pacing (zero `timeBeginPeriod`), dedicated `HWND_MESSAGE` Raw Input thread (`GetRawInputBuffer` + `RIDEV_REMOVE`), same-monitor fullscreen auto-hide, OBS Capture Stage window, invisible helper HWND (tray, hotkeys, `TaskbarCreated`, KB135788-safe menus), SID-locked Named Pipe server | `pet-core`, `pet-render`, `windows` (zero GPU/UIA DLLs) |
| `crates/cat-designer` | Ephemeral Windows GUI (`cat-designer.exe`): CPU-rendered `egui` + embedded `pet-render` preview over Named Pipe IPC (0 MB retained memory when closed; offline draft recovery on disconnect) | `pet-core`, `pet-render`, `egui`, `windows` (zero GPU DLLs) |

## Quick Start

```powershell
# Ensure Rust GNU toolchain is on PATH
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

# Run unit tests + golden PNG & black-cat contrast tests
cargo test --workspace

# Build optimized release binaries
cargo build --release --workspace

# Launch the resident desktop overlay
Start-Process .\target\release\cat-overlay.exe
```

## Controls & Defaults

* **Unlocked mode (default on first launch):**
  * **Left-drag** anywhere on the cat or desk to reposition (position-only move, saved per monitor).
  * **Rub pointer back and forth over the cat's head** to pet it (eye squint + ear relax + squash rebound).
  * **Right-click** the cat or click the system tray icon to open the menu or launch **Cat Designer**.
* **Hotkeys (configurable in `%APPDATA%\DesktopCat\config.json`):**
  * `Ctrl+Alt+L` — Toggle Click-Through Lock (disables dragging & head patting).
  * `Ctrl+Alt+P` — Toggle Manual Privacy Mode (shuts eyes, curls tail over keyboard, suppresses pop-ups).
  * `Ctrl+Alt+D` — Open **Cat Designer** (`cat-designer.exe`).
* **Escape hatch:** Re-launching `cat-overlay.exe` while already running unlocks click-through, clamps the cat onto the primary monitor, and opens Cat Designer.
* **Privacy by construction:** Default typing is **Zone-Anonymized** (side-matched paw rhythm without revealing exact key identity). Opt into **Showcase Mode** inside Cat Designer or the tray menu for exact 1:1 key lighting, keycap legends, and floating white keycap pop-ups.
* **OBS Streaming:** Enable **OBS Capture Stage** to open a standard non-topmost `"Desktop Cat - Capture Stage"` window with configurable chroma-key backdrop (suspends fullscreen auto-hide while active).

## Data & Diagnostics Paths

* Config & custom profiles: `%APPDATA%\DesktopCat\config.json` and `%APPDATA%\DesktopCat\profiles\<uuid>.json`
* Redacted rolling logs (zero keycodes, characters, or window titles): `%LOCALAPPDATA%\DesktopCat\logs\overlay.log`
