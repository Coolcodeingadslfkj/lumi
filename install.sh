#!/bin/sh
# Installs Lumi for the current user (no sudo needed).
#   ./install.sh            install (uses ./lumi if present, otherwise builds from source)
#   ./install.sh uninstall  remove the app (your courses and settings are kept)
set -e
cd "$(dirname "$0")"
BIN="$HOME/.local/bin"
APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICONS="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/256x256/apps"
AUTOSTART="${XDG_CONFIG_HOME:-$HOME/.config}/autostart/lumi-reminders.desktop"

if [ "$1" = "uninstall" ]; then
    rm -f "$BIN/lumi" "$APPS/lumi.desktop" "$ICONS/lumi.png" "$AUTOSTART"
    echo "Lumi removed. Your data is still in ${XDG_DATA_HOME:-$HOME/.local/share}/lumi (delete it to start fresh)."
    exit 0
fi

if [ -x ./lumi ]; then
    SRC=./lumi
else
    [ -x "$HOME/.cargo/bin/cargo" ] && PATH="$HOME/.cargo/bin:$PATH"   # rustup's default location
    command -v cargo >/dev/null || { echo "No prebuilt ./lumi here and Rust isn't installed. Get Rust from https://rustup.rs or download a release."; exit 1; }
    cargo build --release
    SRC=target/release/lumi
fi
ICON=assets/icon.png; [ -f "$ICON" ] || ICON=lumi.png
DESKTOP=packaging/lumi.desktop; [ -f "$DESKTOP" ] || DESKTOP=lumi.desktop

mkdir -p "$BIN" "$APPS" "$ICONS"
install -m 755 "$SRC" "$BIN/lumi"
install -m 644 "$ICON" "$ICONS/lumi.png"
sed "s|^Exec=.*|Exec=$BIN/lumi|" "$DESKTOP" > "$APPS/lumi.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$APPS" 2>/dev/null || true
# KDE (Kubuntu) refreshes its app menu cache with kbuildsycoca
(command -v kbuildsycoca6 >/dev/null && kbuildsycoca6 >/dev/null 2>&1) || (command -v kbuildsycoca5 >/dev/null && kbuildsycoca5 >/dev/null 2>&1) || true

echo "Lumi installed. Open it from your app menu, or run: $BIN/lumi"
command -v notify-send >/dev/null || echo "Tip: install libnotify-bin for study reminders (sudo apt install libnotify-bin)."
