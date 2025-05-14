# TerminalDrome - Terminalbasierter Navidrome Client

![Rust](https://img.shields.io/badge/Rust-1.70+-orange)
![Platform](https://img.shields.io/badge/Platform-ppc64%20%7C%20aarch64%20%7C%20x86__64-lightgrey)

**TerminalDrome** ist ein schlanker Subsonic-API-kompatibler Musikclient für Terminal-Umgebungen, optimiert für ältere Hardware wie den PowerMac G5. 

![TerminalDrome Screenshot](terminaldrome.png)

## Stand:
* Aktualisiert die Anzeige nicht, wenn der nächste Song automatisch abgespielt wird.

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

### Binaries (empfohlen)
Laden Sie vorkompilierte Versionen für Ihr System von den [Releases](https://github.com/thafaker/termnavi/releases):

```bash
# Beispiel für PowerMac G5 (ppc64)
wget https://github.com/thafaker/termnavi/releases/download/v0.1.0/terminaldrome-ppc64
chmod +x terminaldrome-ppc64
./terminaldrome-ppc64```

Aus den Quellen

### Für benutzerdefinierte Kompilierung:

Rust installieren:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
Repository klonen und bauen:
bash
git clone https://github.com/thafaker/termnavi.git
cd termnavi
cargo build --release
Binary finden Sie unter:
bash
target/release/terminaldrome
Paketinstallation (Community-Maintained)```

### ⚠️ Noch nicht verfügbar - Helfen Sie mit bei der Erstellung von:

AUR-Paket für Arch
Homebrew-Tap für macOS
DEB-Paket für Debian
RPM-Paket für Fedora
⚙️ Konfiguration

Erstellen Sie nano config.toml eine Konfigurationsdatei im Verzeichnis:

### config.toml

Fügen sie folgendes hinzu und aktualisieren sie entsprechend:

```[server]
url = "https://dein.navidrome.server"
username = "dein_benutzername"
password = "dein_passwort"```


### ❗ Bekannte Einschränkungen

Alpine Linux benötigt musl-dev für Kompilierung
Gentoo erfordert sys-devel/clang-14+
BSD-Systeme benötigen manuelle Ports