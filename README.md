# Speedy

Speedy privately counts keyboard activity and shows daily, hourly, and live typing analytics. It records only key-down counts, never which keys were pressed, and stores everything locally.

Speedy includes a terminal dashboard and an Omarchy 4 bar plugin. The widget shows today's count, reports live keys per minute in its tooltip, starts the recorder with the shell, and opens the dashboard when clicked.

## Install On Omarchy

Run the installer:

```bash
bash <(curl -fsSL https://raw.githubusercontent.com/4m1z/speedy/main/install.sh)
```

The installer:

- Adds and enables `io.github.4m1z.speedy` with Omarchy's plugin manager.
- Installs the latest verified x86_64 release to `~/.local/bin/speedy`.
- Falls back to a local Rust build when a release is not available.
- Starts the background recorder.

If your account is not in the `input` group, the installer prints the one command needed to grant keyboard-device access. Log out and back in after changing group membership.

## Widget Controls

- Left click: open or focus the Speedy dashboard.
- Right click: start the recorder.
- Middle click: refresh the widget.

Move it like any other Omarchy widget:

```bash
omarchy bar move io.github.4m1z.speedy --section right
```

## Terminal Usage

```text
speedy           Open the dashboard and start recording
speedy --start   Start the background recorder
speedy --status  Print JSON status for integrations
speedy --stop    Stop the background recorder
```

The recorder continues after the dashboard closes. Data is stored in `~/.local/share/keypulse/keypulse.db` to preserve data from Speedy's earlier Keypulse name.

## Update

Run the installer again. It updates both the git-managed plugin and the installed binary:

```bash
bash <(curl -fsSL https://raw.githubusercontent.com/4m1z/speedy/main/install.sh)
```

## Uninstall

```bash
speedy --stop
omarchy plugin remove io.github.4m1z.speedy
rm ~/.local/bin/speedy
```

The database is intentionally retained. Remove `~/.local/share/keypulse` separately if you also want to erase all recorded counts.

## Develop

```bash
cargo test --locked
cargo run
omarchy plugin validate .
```

For local plugin development, install the checkout as a git-managed plugin and enable it (edits require a copy or reinstall because symlinks fail `omarchy plugin validate`):

```bash
omarchy plugin add file://$PWD --yes
omarchy plugin enable io.github.4m1z.speedy
```

Or copy the checkout to `~/.config/omarchy/plugins/io.github.4m1z.speedy` for hot-reload of QML edits.

## Publish A Release

The release workflow builds, tests, and publishes the Linux x86_64 binary used by the installer. Update the versions in `Cargo.toml` and `manifest.json`, then push a matching tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

More background: [Building Speedy](https://amirahmadzadeh.com/blog/speedy).
