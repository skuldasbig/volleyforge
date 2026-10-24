# 🏐 VolleyForge

> **Die authentische 1-zu-1 Reimplementierung des Klassikers *Blobby Volley* (2000/2001) in modernem Rust.**  
> *Genau das gleiche Feeling wie damals in der Kindheit.*

<p align="center">
  <a href="https://skuldasbig.github.io/volleyforge/">
    <strong>🎮 Hier direkt im Browser spielen (WebAssembly)</strong>
  </a>
</p>

<p align="center">
  <img src="screenshots/menu.png" alt="VolleyForge Hauptmenü" width="49%">
  <img src="screenshots/game.png" alt="VolleyForge Gameplay" width="49%">
</p>

---

## 📖 Über das Projekt

Wer um die Jahrtausendwende einen Computerraum in der Schule besucht oder Nachmittage auf LAN-Partys verbracht hat, kennt es: **Blobby Volley**. Zwei Gummibläschen, ein Volleyball, ein Strand und unzählige packende Duelle gegen Freunde oder die kultigen KI-Bots *Adam*, *Bert* und *Carl*.

**VolleyForge** ist ein unabhängiger, moderner Clean-Room-Port des Originals von 2000/2001 (Version 1.7.4). Das Ziel: **100 % Gameplay-Fidelity**. Jeder Bewegungsschritt, jede Kollision mit dem Netz, die Ballphysik, das Sprungverhalten, die Farbtöne und der Sound wurden pixel- und impulsgenau rekonstruiert – ganz ohne veraltete Delphi-/DirectX7-Abhängigkeiten, flüssig und plattformunabhängig spielbar.

---

## 🔬 Wie ist VolleyForge entstanden?

VolleyForge ist kein bloßer Nachbau „nach Augenmaß“, sondern das Resultat echter Rekonstruktionsarbeit:

* 🔍 **Reverse Engineering:** Die originale `volley.exe` wurde mit dem **REA-Plugin** (*Reverse Engineer Anything*) analysiert, worüber **Ghidra** gesteuert wurde, um Physik, Bitmaps, LZW-Container und Konstanten exakt aus dem Maschinencode zu entschlüsseln.
* 🤖 **Iteratives Pair-Programming (definitiv kein One-Shot!):** Die Reimplementierung entstand nicht auf magischen Knopfdruck, sondern in intensiver, manuell gelenkter Schritt-für-Schritt-Arbeit mit **MiniMax AI (M3.1 Flash)** – mit viel händischem Nachjustieren, Debuggen und Feinschliff (ist ja schließlich kein Claude Opus :D).
* ☕ **Der Treibstoff:** Jede Menge Kaffee, Geduld und Zeit, um die unzähligen Details und Eigenheiten des Originals pixel- und tickgenau nachzubauen.

---

## ⚠️ Wichtig: Original-Assets & Copyright

Aus Respekt vor dem Urheberrecht und zur Wahrung des Clean-Room-Prinzips enthält dieses Repository **keine urheberrechtlich geschützten Original-Dateien** (Grafiken, Sounds oder Texte). 

Um VolleyForge zu spielen oder zu bauen, werden die Original-Asset-Container benötigt:
- `graph.pak` (Sprites, Hintergründe, Schriftarten, Menügrafiken)
- `sound.pak` (Original-Soundeffekte)
- `text.pak` (Lokalisierungen: Deutsch, Englisch, Französisch)

### 📥 Woher bekomme ich die Originaldateien?
Das Originalspiel samt Assets ist auf [archive.org](https://archive.org/) archiviert:
* ⬇️ **Direkter Download:** [blobby.zip herunterladen](https://archive.org/download/blobbyvolley_202102/blobby.zip) (ca. 800 KB, enthält `graph.pak`, `sound.pak`, `text.pak`)
* 🔗 **Archive-Detailseite:** [Blobby Volley auf Archive.org ansehen](https://archive.org/details/blobbyvolley_202102)

---

## 🚀 Spiel starten / Einrichten

> 🌐 **Direkt im Browser spielen:** Dank WebAssembly kannst du VolleyForge direkt im Webbrowser ohne Installation spielen:  
> 👉 **[https://skuldasbig.github.io/volleyforge/](https://skuldasbig.github.io/volleyforge/)**  
> *(Beim ersten Aufruf einfach `blobby.zip` oder die drei `.pak`-Dateien per Drag & Drop ins Browserfenster ziehen – fertig!)*  
>  
> 💡 **Desktop-Downloads:** Unter **Releases** findest du fertige Pakete für alle gängigen Plattformen: **Windows** (x86_64, 32-Bit x86, ARM64 als `.exe` & `.zip`), **macOS** (Universal `.dmg` & `.zip` für Apple Silicon & Intel) sowie **Linux** (`.AppImage` & `.tar.gz` für x86_64 und ARM64 / Raspberry Pi).

Du kannst VolleyForge auf zwei einfache Arten mit den Assets versorgen:

### Option A: Container neben das Programm legen (Empfohlen)
1. Lade die [blobby.zip](https://archive.org/download/blobbyvolley_202102/blobby.zip) von Archive.org herunter und entpacke das Archiv.
2. Kopiere die drei Dateien **`graph.pak`**, **`sound.pak`** und **`text.pak`** **neben die Programmdatei**:
   ```text
   volleyforge/
   ├── volleyforge.exe
   ├── graph.pak
   ├── sound.pak
   ├── text.pak
   └── ...
   ```
   Beim Start prüft das Spiel, ob alle drei da sind, und entpackt sie mit einem
   Ladebildschirm („Initialisierung → Entpacke Graphiken → Entpacke Zeichensatz →
   Sounds → Texte"). Fehlt eine Datei, erscheint eine Meldung mit dem erwarteten
   Ordner — mit <kbd>Enter</kbd> beendet sich das Spiel.
3. Starte das Spiel (oder starte die kompilierte `volleyforge.exe`).

### Option B: VolleyForge-EXE direkt ins Originalverzeichnis legen
Du kannst die fertig kompilierte **`volleyforge.exe`** auch einfach direkt in den entpackten Ordner des Originalspiels (wo die `.pak`-Dateien bereits liegen) einfügen und von dort aus starten.

---

## 🎮 Steuerung

Die Steuerung entspricht exakt dem gewohnten Original:

| Steuerung | Spieler 1 | Spieler 2 |
| :--- | :--- | :--- |
| **Links** | <kbd>A</kbd> | <kbd>←</kbd> (Pfeiltaste links) |
| **Rechts** | <kbd>X</kbd> | <kbd>→</kbd> (Pfeiltaste rechts) |
| **Sprung** | <kbd>C</kbd> | <kbd>↑</kbd> (Pfeiltaste oben) |

### Maussteuerung
* Zielposition: Horizontale Mausbewegung steuert den Blob.
* Sprung: **Linke Maustaste** drücken.

### Allgemeine Tasten
* **<kbd>F11</kbd>**: Nahtloses Umschalten zwischen Fenster- und Vollbildmodus (inkl. dezentem Hinweis im Hauptmenü)
* **<kbd>ESC</kbd>**: Pause / Menü / Zurück

---

## ✨ Features

- 🏐 **Präzise 1:1 Physik:** Exakte Ballintegration, Netzkollisionen, Sub-Stepping und Hitboxen.
- 🎨 **Originale Canvas-Pipeline:** 800×600-Canvas, Tag-/Nachtmodus für Menüs und Original-Farbtönungen.
- 🤖 **Kultige KI:** Vollwertige Nachbildung der Gegner *Adam* (leicht), *Bert* (mittel) und *Carl* (schwer).
- 🔊 **Sound-Support:** Echte Retro-WAV-Soundausgabe über ein leichtgewichtiges modernes Audio-Backend.
- 🖥️ **Modernes Drumherum:** Schnelles, sauberes Rust-Fundament mit `egui`/OpenGL (`glow`), flexibler Fenstermodus oder echter Vollbildmodus (<kbd>F11</kbd>) mit scharfer nativer Skalierung.
- 🛠️ **Integrierte Diagnose-Tools:** CLI-Befehle für Selbsttests (`--selftest`) und Asset-Prüfung (`--dump-assets`).

---

## 🛠️ Selbst kompilieren

### Voraussetzungen
* [Rust](https://www.rust-lang.org/) (aktuelle Stable-Version, Edition 2024)
* Die 3 `.pak`-Dateien **im Repository-Root** (sie sind nicht eingecheckt — der
  Build kopiert sie neben das Binary in `target/debug` bzw. `target/release`)

### Bauen & Ausführen
```bash
# Desktop-Version (Windows / Linux)
cargo run --release

# Webversion (WebAssembly im Browser ausführen)
cargo install trunk
trunk serve --release
```

> Aus Urheberrechts- und Clean-Room-Gründen enthält das kompilierte `.wasm`-Modul **keine** eingebetteten Original-Assets.
> Beim ersten Start im Browser zieht der Spieler einfach die originale `blobby.zip` (oder die drei `.pak`-Dateien) per Drag & Drop
> ins Spielfenster – das Spiel entpackt die Dateien clientseitig im RAM und kann sie optional im `localStorage` speichern.

---

## ⚖️ Lizenz & Rechtlicher Hinweis

VolleyForge ist ein reines Fan- und Erhaltungsprojekt. Es steht in keiner offiziellen Verbindung zu den ursprünglichen Entwicklern von *Blobby Volley* (Daniel S. / Silvio M.). Alle Markennamen, Assets und Verweise auf das Originalspiel verbleiben im Besitz der jeweiligen Rechteinhaber. Der Quellcode von VolleyForge wurde unabhängig durch Clean-Room-Reverse-Engineering rekonstruiert.

