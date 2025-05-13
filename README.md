# TerminalDrome - Terminalbasierter Navidrome Client

![Rust](https://img.shields.io/badge/Rust-1.70+-orange)
![Platform](https://img.shields.io/badge/Platform-ppc64%20%7C%20aarch64%20%7C%20x86__64-lightgrey)

**TerminalDrome** ist ein schlanker Subsonic-API-kompatibler Musikclient für Terminal-Umgebungen, optimiert für ältere Hardware wie den PowerMac G5. 

![TerminalDrome Screenshot](terminaldrome.png)

## ✨ Features
- **Vintage-optimiert**: Läuft selbst auf 20+ Jahre alter Hardware (PowerPC G5)
- **Ressourcensparend**: <5MB RAM-Verbrauch, keine GPU-Anforderungen
- **Sofortstart**: Keine komplexen Abhängigkeiten, nur MPV benötigt
- **Smartes Playback**:
  - Fortschrittsbalken mit Echtzeit-Update
  - Automatische Albumwiedergabe nach Titelauswahl
  - Zustandserhaltung zwischen Sitzungen
- **Intuitive TUI**:
  - Drei-Panel-Interface (Künstler → Alben → Titel)
  - Farbige Statusanzeigen
  - Tastaturgesteuerte Navigation

## 🖥️ Kompatibilität
| System          | Arch     | Status      |
|-----------------|----------|-------------|
| PowerMac G5     | ppc64    | ✅ Stabil   |
| Raspberry Pi 4  | aarch64  | ✅ Stabil   |
| Moderne Laptops | x86-64   | ✅ Getestet |

## 🚀 Installation
1. **Voraussetzungen**:
   ```bash
   # Debian/Ubuntu
   sudo apt install mpv

   # Arch Linux
   sudo pacman -S mpv
Kompilieren:
bash
git clone https://github.com/thafaker/termnavi.git
cd termnavi
cargo build --release
Konfiguration (~/.config/termnavi/config.toml):
toml
[server]
url = "https://dein.navidrome.server"
username = "dein_benutzername"
password = "dein_passwort"
⌨️ Bedienung

Tastatur	Aktion
↑/↓	Navigation
←	Zurück zur vorherigen Ansicht
→ / Enter	Auswahl bestätigen
Leertaste	Play/Pause
q	Beenden
🔧 Aktueller Entwicklungsstand

Stabil implementiert:
✅ Automatische Playlist-Fortsetzung
✅ Echtzeit-Player-Status
✅ Fehlerresistente MPV-Integration
✅ Zustandsspeicherung zwischen Sessions
Geplante Features:
🔲 Playlist-Verwaltung
🔲 Suche
🔲 Themensupport
🛠️ Mitentwickeln

Willkommen sind Beiträge zu:

UI-Verbesserungen: Erweiterte Ratatui-Komponenten
Performance: Speichernutzung auf <2MB reduzieren
Dokumentation: Deutsche/Englische Bedienungsanleitung
Starter Issues:

Implementierung einer Suchleiste
CI/CD für PowerPC-Builds
Alpine Linux-Paket erstellen
📜 Lizenz

MIT License - Details siehe LICENSE