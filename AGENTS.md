# AGENTS.md — Arbeitsregeln für VolleyForge

VolleyForge ist ein **unabhängiger** Port von *Blobby Volley* (Shareware 2001,
Delphi/VCL). Alles wurde aus `volley.exe` rekonstruiert; kein Original-Quellcode
und kein fremder Port wurden konsultiert.

| Was | Wo |
|---|---|
| Quellcode | `src/` (22 Module) |
| Original-Assets | `graph.pak`, `sound.pak`, `text.pak` — **nicht eingecheckt**, liegen neben der EXE (der Build kopiert sie aus dem Repo-Root nach `target/`) |
| **Wissensbasis** | `../WISSENSSTAND.md` — Teil 0 = Dekompilat-Landkarte mit allen Konstanten |
| Konzept | `../KONZEPT.md` — Architektur, Canvas-Pipeline, Spielregeln |
| Dekompilat | `../blobb/re_out/decompiled/*.c` — 83 Ghidra-Ausgaben |
| RE-Werkzeug | REA (MCP) mit Ghidra 12.1.4 — für offene Detailfragen |

> Die beiden Dokumente liegen **außerhalb** dieses Repos und sind deshalb nicht
> versioniert. Vor einem öffentlichen Upload müssen sie mitentschieden werden.

## Harte Regeln

1. **Keine erfundenen Zahlen.** Jede Konstante stammt aus `WISSENSSTAND.md`
   Teil 0 oder direkt aus dem Dekompilat. Was im Original nicht gefunden wurde,
   wird im Code **und** im Wissenstand als Annahme markiert — nicht stillschweigend
   plausibel gewählt.
2. **Im Dekompilat nachsehen, nicht in der Zusammenfassung.** `WISSENSSTAND.md`
   ist eine Landkarte, kein Ersatz für den Quellcode. Beim Ändern von Physik oder
   Steuerung die betreffende `.c` unter `../blobb/re_out/decompiled/` lesen —
   die Zusammenfassung war an mehreren Stellen falsch (Schwerkraft-Vorzeichen,
   Tint-Formel, Schatten-Helligkeit).
3. **Bottom-up achten.** Das Original zeichnet in einem Canvas von unten nach
   oben; der Boden liegt bei `y = 0x1080` (132 px) = top-down 468. Original-
   Koordinaten auf den Bildschirm zu rechnen heißt: erst `layout::to_top_down()`.
4. **Originalgrafik.** Alle Bilder kommen aus `graph.pak`. Keine gezeichneten
   Ersatzgrafiken, keine erfundenen Farben.
5. **Fidelity vor Politur.** Wenn Mockup und Original sich widersprechen, gewinnt
   das Original — und die Abweichung wird dokumentiert.

## Ablauf einer Änderung

1. Änderung umsetzen.
2. `cargo build` — keine Fehler, keine neuen Warnungen.
3. **Abnahme:** `volleyforge --shot <screen>` rendert headless nach `shot_<screen>.png`.
   Für den Spielbildschirm: gegen `mockup_game.png` pixelweise vergleichen
   (0,4 % Abweichung sind der aktuelle Stand, siehe WISSENSSTAND Teil 7 Punkt 1).
   Für die Physik: `volleyforge --selftest N`.
4. `WISSENSSTAND.md` / `KONZEPT.md` aktualisieren, wenn sich Konstanten,
   Erkenntnisse oder der Umsetzungsstand ändern.
5. **Commit** (siehe unten).

## Commit-Routine

- **Nach jeder großen Änderung wird ein Commit angelegt.** Festgelegt vom
  Nutzer am 2026-10-22.
- Eine inhaltlich abgeschlossene Änderung pro Commit. Refactorings und
  Verhaltensänderungen nicht vermischen.
- Betreff: `typ(bereich): Kurzbeschreibung` in Kleinschreibung, imperative
  Formulierung. Typen: `feat`, `fix`, `refactor`, `docs`, `chore`, `test`.
- Der Body nennt **was** geändert wurde und **wie es geprüft** wurde
  (Messwerte, Shot-Vergleiche, Testergebnisse) — nicht nur das Was.
- Vor dem Commit `git status` prüfen. `target/`, `assets_dump/` und `*.png` sind
  in `.gitignore` und dürfen nicht eingecheckt werden.
- **Push nur auf ausdrückliche Anweisung** des Nutzers. Ein Commit bleibt
  sonst lokal.

## Projektstruktur

```
src/
  main.rs      CLI: --shot, --verify, --dump-assets, --selftest, --font
  app.rs       egui-Shell, Screen-Automat, Canvas→Textur, Eingabelayer
  render.rs    Spielbildschirm (Originalgrafik)          → Canvas
  screens/     7 Menübildschirme + Dialoge              → Canvas
  canvas.rs    800x600-Canvas mit den Original-Operationen
  fonts.rs     55 Font-Slots, 20 px Abstand, Schräglage
  layout.rs    Layout-Konstanten, to_top_down(), Tint
  court.rs     Geradekoordinaten, Fixed-Point, Konstanten
  ball.rs      Ballzustand + Integration (2 Substeps, FUN_004441D0)
  player.rs    Spielerzustand, Projektion, Touch-Zähler
  rules.rs     Rally-State-Machine, Trefferregel, CPU
  settings.rs  volleyforge.vf, 5 Steuerungsmodi, 8 Teamfarben
  pak.rs       LZW-Container-Format
  assets.rs    Laden der drei .pak-Container
```

**Ein Canvas für alles** (KONZEKT 2.1): Menü *und* Spiel rendern in dieselbe
800×600-Fläche, die als Textur in egui geladen wird. Nur so greifen
Originalfont, Nachtmodus und das Original-Layout überall.

## Steuerung (Original)

| | |
|---|---|
| Tastatur Spieler 1 | **A** links, **X** rechts, **C** Sprung |
| Tastatur Spieler 2 | Pfeiltasten ← → ↑ |
| Maus | Ziel-x = `mouse_x / 2 + player_index * 400`, **linke Maustaste = Sprung** |
| Computer | Adam (leicht), Bert (mittel), Carl (schwer) |

Aus dem Original-Hilfetext (`@help`): „If you set mouse control, clicking the
left mouse button results in a jump."
