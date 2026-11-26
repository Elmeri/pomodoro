# Pomo

A fast, single-binary Pomodoro timer and focus tracker for macOS—combining a native menu bar app (`Pomo.app`) with a lightweight terminal CLI (`pomo`), backed by a local SQLite database.

## Features

- **Native macOS Menu Bar App (`Pomo.app`):** Lives quietly in your top menu bar with a monochrome Dark/Light Mode template icon and zero Dock clutter (`LSUIElement`).
- **Floating Custom Session Popup:** Configure custom durations (1–90 minutes), pick from recent project chips, pause/resume active sessions, and view today's focus totals.
- **Stealth Countdown Mode:** Toggle **Show Countdown in Bar** to hide ticking seconds in the menu bar while keeping live session time inside the dropdown menu (persisted across restarts).
- **Terminal CLI & Stats:** Start countdowns directly in your terminal and query daily or all-time focus totals by project.
- **Local SQLite Persistence:** All sessions and preferences are stored locally in `~/Library/Application Support/pomo/pomo.db`.

---

## Installation

### Option 1: Prebuilt macOS Universal Release (Recommended)

Download the latest release from the [Releases](https://github.com/YOUR_USERNAME/pomo/releases) page:

#### Install `Pomo.app` (Menu Bar App)
1. Download `Pomo-vX.Y.Z-universal-macos.zip` and extract it.
2. Move `Pomo.app` to `/Applications`:
   ```bash
   mv Pomo.app /Applications/
   ```
3. Remove the macOS quarantine attribute (required because the release binary is ad-hoc signed rather than Apple-notarized):
   ```bash
   xattr -cr /Applications/Pomo.app
   ```
4. Launch the app:
   ```bash
   open /Applications/Pomo.app
   ```

#### Optional: Expose the `pomo` CLI Globally
You can symlink the executable inside `Pomo.app` so terminal commands work from anywhere without installing a separate binary:
```bash
sudo ln -sf /Applications/Pomo.app/Contents/MacOS/pomo /usr/local/bin/pomo
```
*(Alternatively, download `pomo-vX.Y.Z-universal-apple-darwin.tar.gz`, extract `pomo`, and place it in a directory on your `$PATH`.)*

---

### Option 2: Build from Source

#### Prerequisites
- Rust toolchain (`rustup` / `cargo` 1.75+)
- [`cargo-bundle`](https://github.com/burtonageo/cargo-bundle) (for packaging `Pomo.app`)

#### Build & Install `Pomo.app`
```bash
git clone [https://github.com/YOUR_USERNAME/pomo.git](https://github.com/YOUR_USERNAME/pomo.git)
cd pomo

cargo install cargo-bundle --locked
cargo bundle --release --format osx

rm -rf /Applications/Pomo.app
ditto target/release/bundle/osx/Pomo.app /Applications/Pomo.app
open /Applications/Pomo.app
```

#### Build CLI Only
```bash
cargo install --path .
```

---

## CLI Usage

```bash
# Start a 25-minute default Pomodoro session
pomo start

# Start a 45-minute session tagged to a specific project
pomo start -p writing -m 45

# View focus statistics
pomo stats

# Launch the menu bar tray directly from the terminal
pomo tray
```

---

## Data Storage

- **Database path:** `~/Library/Application Support/pomo/pomo.db`
- **Tables:**
  - `sessions` — logs completed project name, duration in minutes, and ISO-8601 timestamp.
  - `settings` — stores menu bar preferences (such as `show_countdown_in_bar`).