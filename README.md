# TerminalDrome - Terminalbasierter Navidrome Client

![Rust](https://img.shields.io/badge/Rust-1.70+-orange)
![Platform](https://img.shields.io/badge/Platform-ppc64%20%7C%20aarch64%20%7C%20x86__64-lightgrey)

**TerminalDrome** ist ein schlanker Subsonic-API-kompatibler Musikclient für Terminal-Umgebungen, optimiert für ältere Hardware wie den PowerMac G5. 

![TerminalDrome Screenshot](terminaldrome.png)

## Stand:
* absolute pre Alpha!!!
* Scrobbling zu last.fm und listen.brainz funktioniert über Navidrome
* Titel aktualisiert sich beim Spielen. Ist ein Song zu Ende wechselt er automatisch zum nächsten Song und zeigt dies auch an. Update Zeit und Song funktioniert.
* Wir sind jetzt in Beta.

## ✨ Hauptfeatures von TerminalDrome

1. Navidrome-Integration
	* Verbindung zu deinem Navidrome-Server (HTTPS-erzwungen)
	* Unterstützt alle Subsonic-API-Endpoints (Artists, Albums, Songs)
2. TUI (Terminal UI) mit 3-Spalten-Design
	* Artists → Albums → Songs
	* Intuitive Navigation mit Pfeiltasten
	* Farbige Hervorhebungen (aktive Songs, Auswahl, Status)
3. Musikwiedergabe
	* MPV-Integration (lautlos im Hintergrund)
	* Automatischer Übergang zum nächsten Song (Playlist-Modus)
	* Play/Pause mit Leertaste
	* Fortschrittsbalken und Laufzeitanzeige
4. Last.fm-Scrobbling
	* Automatisches Scrobbeln bei ~50% der Songdauer
	* Korrekte Zeitstempel (Unix-Millisekunden)
	* Vermeidung von Duplikaten (via current_scrobble_sent-Flag)
5. Persistenz
	* Speichert den letzten Zustand (state.json):
	* Aktueller Künstler/Album/Song
	* Scroll-Positionen
	* Now-Playing-Index
	* Stabile MPV-Kommunikation
	* Unix-Socket für Echtzeit-Updates (Playlist-Position, Zeit)
	* Behandelt Playlist-Ende korrekt
	* Minimalistische Statusleiste
	* Anzeige des aktuellen Songs + Album/Artist
	* Klare Fehlermeldungen (z. B. bei Verbindungsproblemen)

## 🔧 Technische Highlights

* Rust-basiert (schnell & sicher)
* Async/await für non-blocking I/O
* Atomic Operations für Thread-sicheren Status (MPV ↔ UI)
* TOML-Konfiguration (Server-URL, Credentials)

## 🚀 Roadmap-Ideen (optional)

* Suche (Filterfunktion in Listen)
* Shuffle/Repeat-Modi
* Cover-Art (via Sixel oder ASCII-Art)
* Theme-Unterstützung (farbige Schemes)

## 🖥️ Kompatibilität
| System          | Arch     | Status      |
|-----------------|----------|-------------|
| PowerMac G5     | ppc64    | ✅ Stabil   |
| Raspberry Pi 4  | aarch64  | ✅ Stabil   |
| Moderne Laptops | x86-64   | ✅ Getestet |
| Mac mini M4 | arm64   | ✅ Getestet |

## 📦 Installation

### Voraussetzungen
- **MPV** (mind. 0.34+)
- **Rust Toolchain** (nur für Eigenkompilierung)

#### Paketmanager-Installation MPV:
| Distribution | Befehl |
|--------------|--------|
| Ubuntu/Debian | `sudo apt install mpv` |
| Arch Linux/Manjaro | `sudo pacman -S mpv` |
| Fedora/RHEL | `sudo dnf install mpv` |
| openSUSE | `sudo zypper install mpv` |
| macOS (Homebrew) | `brew install mpv` |
| Void Linux | `sudo xbps-install mpv` |

#### Build with cargo in path

```bash
git clone https://github.com/thafaker/termnavi.git TerminalDrome

cd TerminalDrome

cargo build --release
```

### Binaries (empfohlen)

Laden Sie vorkompilierte Versionen für Ihr System von den [Releases](https://github.com/thafaker/termnavi/releases):
