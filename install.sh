#!/bin/sh
# Baut BoxQu und installiert es für den aktuellen Nutzer (ohne root) samt Startmenü-Eintrag.
# Entfernen: die Dateien löschen, die `--install` ausgibt (Programm, Icon, Startmenü-Eintrag).
set -e
cd "$(dirname "$0")"

cargo build --release
# Programm, Icon und Startmenü-Eintrag (Texte aus qu.toml) legt die App selbst an.
./target/release/boxqu --install
