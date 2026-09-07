# confsync

Desktop application that backs up and restores Linux configuration files in a
git repository, with a tray agent that watches for changes. Rust + egui.

## Requirements

```bash
# Rust 1.81+
rustup update stable

# Build dependencies (Debian/Ubuntu)
sudo apt install build-essential pkg-config libssl-dev cmake

# Fedora
sudo dnf install gcc-c++ pkgconf-pkg-config openssl-devel cmake
```

## Build and run

```bash
cargo run -p confsync-gui --release
```

Tests (no GUI required):

```bash
cargo test --workspace
```

## Arch Linux package

```bash
cd packaging
makepkg -f                                    # builds the package, runs the tests
sudo pacman -U confsync-*.pkg.tar.zst
```

The package installs:

| | |
|---|---|
| `/usr/bin/confsync` | desktop application |
| `/usr/bin/confsync-agent` | tray agent |
| `/usr/share/applications/confsync.desktop` | application menu entry |
| `/usr/lib/systemd/user/confsync-agent.service` | user service |
| `…/graphical-session.target.wants/confsync-agent.service` | enablement symlink |

Because of the last line the agent ships **enabled**: there is no need to run
`systemctl --user enable`, it starts with the graphical session. To start it
right after installing, without re-opening your session:

```bash
systemctl --user start confsync-agent.service
```

If you do not want the agent, `disable` is not enough — the enablement comes
from the package, so mask it:

```bash
systemctl --user mask confsync-agent.service
```

Remove with `sudo pacman -R confsync`. Settings and the backup repository are
left in place.

An AUR package is prepared under [`packaging/aur/`](packaging/aur/).

## Usage

1. In **Settings**, set the local repository path and, if you have one, the
   remote URL. Remotes authenticate through your ssh-agent key or the git
   credential helper; the application never stores a password.
2. In **Sources**, pick the folders and files to back up. `~/.config` is not
   added as a whole (it holds browser profiles and application state); known
   configuration entries are added individually, and the discovery panel
   measures the rest so you can decide.
3. In **Excludes**, write patterns using gitignore syntax. Cache directories
   and well-known key files are already in the default list.
4. **Back Up Now** scans, copies into the repository and commits. If nothing
   changed, no empty commit is created. When a file needs a decision — a
   suspected secret, or one over the size limit — a review window opens first
   and nothing is written until you confirm.
5. **Overview** lists what changed since the last backup, with each file's
   status and size.
6. In **Restore**, a plan is built first — nothing is written to disk at that
   point. Once you review the list and confirm, files are written, and a copy
   of every overwritten file is kept under
   `~/.local/share/confsync/rollback/`.

## File locations

| Path | Contents |
|---|---|
| `~/.config/confsync/settings.toml` | application settings |
| `~/.local/share/confsync/repo/` | default local git repository |
| `~/.local/share/confsync/rollback/` | safety copies taken before a restore |

## Warning

Secret detection is heuristic, not exact. Before making a remote repository
public, review the skipped-files list in **Overview** and the contents of the
repository itself.

## Agent (tray)

`confsync-agent` runs without a window: it sits in the tray, checks the
sources at a regular interval and notifies you when something changed.

```bash
cargo run -p confsync-agent --release      # run in the tray
confsync-agent --once                      # one check, print the result, exit
confsync-agent --backup                    # one backup, exit
```

The icon colour carries the state: green (everything backed up), blue (pending
changes), orange (your decision needed), red (error), grey (paused). Left
click opens the application; the right-click menu offers checking, backing up
and pausing.

The interval (5 minutes by default) and automatic backup are managed under
**Settings → Agent**; the agent re-reads them every round.

If any file needs a decision, the agent **never backs up on its own** — it
only notifies and leaves the decision to the review window. It also does not
repeat a notification for a change set it has already reported.

### Quiet sources and the daily backup

Some folders change all day long, and being told about them is pure noise.
Mark those sources **quiet** in the Sources tab: changes there never raise a
notification and never turn the tray icon busy. They still show up in the
Overview table, tagged `quiet`.

Pair that with **Settings → Agent → "Once a day, back up and push quietly"**:
at most once every 24 hours the agent takes a normal backup and pushes it, in
silence. Whether the day is up is decided from the age of the last commit, so
a manual backup also counts and no extra state file is needed.

A file awaiting a decision inside a quiet source is not counted either — the
promise of "quiet" is that nothing comes out of it. It simply stays out of the
backup until you include it from the review window.

### Why no instant (inotify) watching?

A full scan takes well under a second for a typical configuration tree
(measured: ~0.4 s for 648 files / 24 MiB). Polling gives the same answer
without managing watches or dealing with editors that write via `rename`.
If latency ever matters, it can be added to the same agent as a trigger.
