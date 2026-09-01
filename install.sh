#!/usr/bin/env bash

set -euo pipefail

readonly REPO_URL="https://github.com/4m1z/speedy.git"
readonly RELEASE_URL="https://github.com/4m1z/speedy/releases/latest/download"
readonly PLUGIN_ID="io.github.4m1z.speedy"
readonly PLUGIN_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/omarchy/plugins/$PLUGIN_ID"
readonly BIN_DIR="$HOME/.local/bin"
readonly BIN_PATH="$BIN_DIR/speedy"
readonly CACHE_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/speedy"

fail() {
  printf 'speedy installer: %s\n' "$*" >&2
  exit 1
}

command -v omarchy >/dev/null || fail "Omarchy is required"
omarchy plugin --help >/dev/null 2>&1 || fail "Omarchy 4.0 or newer is required"
command -v curl >/dev/null || fail "curl is required"

if [[ -e "$PLUGIN_DIR" || -L "$PLUGIN_DIR" ]]; then
  [[ -f "$PLUGIN_DIR/manifest.json" ]] || fail "$PLUGIN_DIR is not a valid Speedy plugin"
  if [[ -d "$PLUGIN_DIR/.git" && ! -L "$PLUGIN_DIR" ]]; then
    omarchy plugin update "$PLUGIN_ID" --yes
  fi
else
  omarchy plugin add "$REPO_URL" --yes
fi

tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT

install_release() {
  [[ $(uname -m) == "x86_64" ]] || return 1

  local archive="speedy-x86_64-unknown-linux-gnu.tar.gz"
  curl -fsSL "$RELEASE_URL/$archive" -o "$tmp_dir/$archive" || return 1
  curl -fsSL "$RELEASE_URL/$archive.sha256" -o "$tmp_dir/$archive.sha256" || return 1
  (cd "$tmp_dir" && sha256sum --check "$archive.sha256") || return 1
  tar -xzf "$tmp_dir/$archive" -C "$tmp_dir"
  install -Dm755 "$tmp_dir/speedy" "$BIN_PATH"
}

install_from_source() {
  if ! command -v cargo >/dev/null; then
    printf 'No release binary is available; installing Rust to build Speedy.\n'
    omarchy pkg add rust
  fi

  mkdir -p "$CACHE_DIR"
  CARGO_TARGET_DIR="$CACHE_DIR/build" cargo build \
    --locked \
    --release \
    --manifest-path "$PLUGIN_DIR/Cargo.toml"
  install -Dm755 "$CACHE_DIR/build/release/speedy" "$BIN_PATH"
}

if install_release; then
  printf 'Installed the latest Speedy release.\n'
else
  printf 'No compatible release binary found; building Speedy from source.\n'
  install_from_source
fi

omarchy plugin enable "$PLUGIN_ID"

"$BIN_PATH" --stop >/dev/null || true
"$BIN_PATH" --start

printf '\nSpeedy is installed and enabled in the Omarchy bar.\n'
if [[ " $(id -nG) " != *" input "* ]]; then
  printf 'Keyboard access still needs setup. Run:\n'
  printf '  sudo usermod -aG input %q\n' "$USER"
  printf 'Then log out and back in.\n'
fi
