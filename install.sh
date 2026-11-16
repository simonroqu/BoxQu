#!/bin/sh
# Baut BoxQu und installiert es für den aktuellen Nutzer (ohne root) samt Startmenü-Eintrag.
# Entfernen: die unten genannten Dateien löschen.
set -e
cd "$(dirname "$0")"

cargo build --release

bin="$HOME/.local/bin/boxqu"
icon="$HOME/.local/share/icons/hicolor/scalable/apps/dev.roqu.BoxQu.svg"
desktop="$HOME/.local/share/applications/dev.roqu.BoxQu.desktop"

install -Dm755 target/release/boxqu "$bin"
install -Dm644 data/dev.roqu.BoxQu.svg "$icon"
mkdir -p "$(dirname "$desktop")"
sed "s|@BIN@|$bin|" data/dev.roqu.BoxQu.desktop > "$desktop"

# Startmenü neu einlesen (falls das Werkzeug da ist).
command -v update-desktop-database >/dev/null && update-desktop-database -q "$(dirname "$desktop")" || true

echo "Installiert: $bin"
echo "Startmenü:   $desktop"
