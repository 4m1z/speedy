# Keypulse

Keypulse is a private Linux terminal dashboard that counts physical key presses from wired and Bluetooth keyboards. It stores aggregate hourly totals in SQLite, then turns them into daily, weekly, and 30-day reports.

## Features

- Global key counting through Linux `evdev`
- Automatic wired, USB, and Bluetooth keyboard discovery
- Hot-plug rescanning every two seconds
- Daily speedometer inspired by an odometer dashboard
- Weekly totals and hourly activity charts
- 30-day trend, hourly distribution, personal best, and streak report
- Transactional SQLite persistence with no key names or typed text
- Key-repeat filtering, so holding a key counts once

## Requirements

- Linux
- A terminal with color and Braille character support
- Rust 1.85 or newer
- Read access to `/dev/input/event*`

## Install

```bash
cargo install --path .
```

This installs the global user command at `~/.cargo/bin/keypulse`.

Linux normally restricts raw input devices. Add your user to the `input` group, then log out and back in:

```bash
sudo usermod -aG input "$USER"
```

Check the device group first if your distribution uses a different one:

```bash
ls -l /dev/input/event*
```

## Run

```bash
keypulse
```

Keypulse counts while it is running. Leave the compact side panel open, or run it in a persistent `tmux` session, to collect the full day.

Bluetooth keyboards need to be paired through the operating system first. Once Linux exposes a paired keyboard as an input device, Keypulse discovers it automatically.

## Controls

| Key | Action |
| --- | --- |
| `1`, `2`, `3` | Open Today, Week, or Report |
| `Left`, `Right` | Change tabs |
| `h`, `l` | Change tabs |
| `q`, `Esc`, `Ctrl+C` | Save and quit |

## Data And Privacy

Statistics are saved in a WAL-enabled SQLite database at:

```text
~/.local/share/keypulse/keypulse.db
```

If `XDG_DATA_HOME` is set, the file is stored at `$XDG_DATA_HOME/keypulse/keypulse.db` instead. Existing data from `stats.json` is migrated automatically on first launch. The database stores one aggregate count per date and hour. Key codes, key names, applications, passwords, and typed text are never stored.

## Omarchy Side Panel

The included Omarchy setup launches Keypulse in a floating 560x620 Alacritty window on the right side of the current workspace. The TUI uses ANSI colors, so its racing palette follows the active terminal theme:

```text
Super + I
```

The shortcut uses Omarchy's launch-or-focus helper, so pressing it again focuses the existing panel instead of opening a duplicate. Its dedicated `Keypulse` window class keeps the floating rules separate from normal terminal windows.

## Troubleshooting

If the header says `[WAITING] no readable keyboard`:

1. Confirm the keyboard appears under `/dev/input/event*`.
2. Confirm your user belongs to the group shown by `ls -l /dev/input/event*`.
3. Log out and back in after changing group membership.
4. Start `keypulse` again without `sudo`.

Some input remapping tools create a virtual keyboard in addition to the physical one. If both devices emit the same event, Linux exposes both streams and counts can be duplicated.

## Development

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```
