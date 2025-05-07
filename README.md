# TermNavi - Navidrome Terminal Client (PowerMac G5 Edition)

![Rust](https://img.shields.io/badge/Rust-1.70+-orange)
![Platform](https://img.shields.io/badge/Platform-ppc64%20%7C%20aarch64-lightgrey)

Ein minimalistischer Terminal-Client für [Navidrome](https://www.navidrome.org/), speziell optimiert für ältere Hardware wie PowerMac G5 (ppc64) und moderne ARM-Systeme (aarch64).

## 🎯 Ziel
- Musikstreaming im Terminal ohne moderne Browser
- Ultra-leichtgewichtige Alternative für Ressourcen-beschränkte Systeme
- Rust-basiert für maximale Performance

## ⚠️ Aktueller Status
**Experimentell** - Grundfunktionen sind implementiert, aber:
- [ ] Playback funktioniert noch nicht stabil
- [ ] Fehlerbehandlung benötigt Verbesserungen
- [ ] UI ist sehr basic

## 🛠️ Kompatibilität
| System       | Arch     | Status      |
|--------------|----------|-------------|
| PowerMac G5  | ppc64    | ✅ Getestet  |
| Mac Mini Mx  | aarch64  | ⚠️ In Arbeit|

## 🚀 Installation
1. Voraussetzungen:
   ```bash
   sudo pacman -S mpv git rustup  # Für Arch Linux ppc64
   rustup target add ppc64-unknown-linux-gnu

2. Bauen:
git clone https://github.com/thafaker/termnavi.git
cd termnavi
cargo build --release

3. Konfiguration (~/.config/termnavi/config.toml):
[server]
url = "https://dein.navidrome.server"
username = "dein_benutzername"
password = "dein_passwort"

🎛️ Bedienung

Tastenkürzel	Aktion
↑/↓	Navigation
P	Titel abspielen
Q	Beenden
💻 Entwicklung

Mithelfen ist willkommen! Besonders bei:

Audio-Playback auf ppc64
Bessere TUI mit Ratatui
Navidrome API-Integration
