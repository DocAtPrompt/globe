# globe

Ein rotierender Globus für dein Terminal — mit echtem Sonnenstand, Tag/Nacht-Grenze in Echtzeit, Stadtlichtern auf der Nachtseite, Atmosphären-Glow, Wolken, sowie Sonne und Mond bei weitem Zoom.

```
cargo run --release
```

`q` oder `Esc` zum Beenden.

## Status

V1 — voll lauffähig mit **echten NASA-basierten Maps** (Blue Marble, Black Marble, MODIS Cloud Cover) bei 2048×1024 für Klassen und Lichter, 1024×512 für Wolken. Sourcen sind Public Domain und werden bei Bedarf via `tools/build_assets.py` neu beschafft (siehe unten).

## Standardansicht

- Kamera auf der **Home-Position** (User-Standort) — beim ersten Start aus der System-Timezone geschätzt
- **Auto-Rotation** in Echtzeit (15°/h, eine Umdrehung pro 24 h)
- Echter Sonnenstand für Datum/Uhrzeit, Tag/Nacht-Grenze wandert sichtbar
- Halbblock-Rendering (`▀`) mit 256 ANSI-Farben — funktioniert in praktisch jedem modernen Terminal

## Tasten

| Taste              | Normalmodus                                | Freeze-Modus                                   |
| ------------------ | ------------------------------------------ | ---------------------------------------------- |
| `←` `→` `↑` `↓`    | Erde drehen (Lon / Lat)                    | Sonne verschieben (Δ zur Live-Position)        |
| `Shift` + Pfeile   | Feinrotation (10× kleiner)                 | Sonnen-Feinrotation                            |
| `+` / `-`          | Zoom rein / raus                           | Zoom rein / raus                               |
| `0`                | Zoom-Reset                                 | Zoom-Reset                                     |
| `h`                | Home-Position                              | Home-Position                                  |
| `s`                | Subsolar-Position (über Äquator)           | Subsolar-Position                              |
| `f`                | **Freeze ein** — Erde + Mond pausieren     | **Freeze aus** — Delta verworfen, alles live   |
| `Space`            | Auto-Rotation toggle                       | —                                              |
| `[` `]`            | Rotations-Speed −/+                        | —                                              |
| `m`                | Modus zyklisch: blocks → ascii → plain     | Modus zyklisch                                 |
| `r`                | Defaults zurück (Position bleibt)          | Defaults + Delta auf null                      |
| `?`                | Hilfe-Overlay                              | Hilfe-Overlay                                  |
| `q` / `Esc`        | Beenden                                    | Beenden                                        |

## CLI

```
globe [-h LAT,LON | --home LAT,LON] [--fps N] [--mode blocks|ascii|plain] [--no-color] [--snapshot]
```

- `--home 48.21,16.37` — Position als Lat/Lon in Grad (überschreibt Timezone-Schätzung)
- `--fps 30` — Frame-Rate-Limit (1–120)
- `--mode plain` oder `--no-color` — Fallback ohne ANSI-Farben
- `--snapshot` — ein Frame in stdout schreiben und beenden (für Tests / CI)

## Render-Modi

1. **blocks** (Default) — `▀`-Halbblöcke mit zwei Farben pro Zelle → höchste effektive Auflösung
2. **ascii** — ASCII-Helligkeitsrampe `" .:-=+*#%@"` mit Farbe je Klasse
3. **plain** — ASCII ohne Farben (für Pipes, log-Dateien, sehr restriktive Terminals)

## Architektur

```
src/
├── main.rs          Event-Loop + Terminal-Setup
├── app.rs           AppState + Key-Handler + Frame-Renderer
├── camera.rs        Kamera-State (lat/lon/distance + Clamping)
├── config.rs        clap CLI-Parsing
├── constants.rs     Zentrale Parameter (FOV, Zoom, Rotation, Schwellwerte)
├── geo.rs           Home-Position aus Timezone oder CLI-Arg
├── moon.rs          Mondbahn (vereinfachte Meeus-Formel) + Phase
├── render.rs        Raycasting, Beleuchtung, ANSI-Farben, Glow, Sterne, Marker
├── sun.rs           Subsolar-Punkt aus Astronomie
├── tui.rs           Frame-Buffer + Diff-Flush
├── vec3.rs          3D-Vektor-Helper + Lat/Lon-Konvertierung
└── world.rs         Klassen-/Lights-/Cloud-Sampling (V1: prozedural)
```

## Tests

```
cargo test --lib
```

94 Tests in 11 Modulen, davon 2 Property-Tests (`proptest`).

## Astronomische Genauigkeit

- **Subsolar-Punkt:** ≈0.01° (vereinfachte NOAA-Formel)
- **Mondbahn:** ≈1° in Lat/Lon (vereinfachte Meeus-Formel — voll-/Neumond-Termine 2026 trifft das Tool auf den Tag genau)
- **Tageslänge:** Erdrotation 24 h pro Umdrehung, gerechnet mit Greenwich Mean Sidereal Time

## Asset-Pipeline (echte NASA-Maps)

Drei Binär-Dateien in `assets/` werden via `include_bytes!` direkt ins Release-Binary gebacken:

| Datei                   | Auflösung   | Quelle                                                              | Größe |
| ----------------------- | ----------- | ------------------------------------------------------------------- | ----- |
| `earth_classes.bin.z`   | 2048×1024   | Blue Marble Day-Map (RGB → 6-Klassen-Heuristik)                     | ~107 KB |
| `earth_lights.bin.z`    | 2048×1024   | Black Marble Stadtlichter (Threshold + 8-bit)                       | ~52 KB  |
| `earth_clouds.bin.z`    | 1024×512    | MODIS Cloud Cover Composite (8-bit Alpha)                           | ~25 KB  |

Lizenz: alle drei sind **NASA Public Domain**, gespiegelt im Three.js-Texture-Repository. Konvertiert mit `tools/build_assets.py` (Python + Pillow). Source-Bilder liegen in `assets/raw/` und werden bei Bedarf vom Script per HTTPS aus dem Mirror geladen.

Neu generieren:
```
python3 tools/build_assets.py
```

Wenn du höher auflösende Maps willst (5400×2700 oder 8192×4096), tausche die URLs in `tools/build_assets.py` und lass das Script laufen — der Rest des Codes bleibt unverändert.

## Bekannte Begrenzungen

- Map-Auflösung 2048×1024 (~20 km/pixel) — größere Inseln (Sizilien, Sri Lanka, Madagaskar) klar erkennbar; sehr kleine (Malta, Mallorca) nur als 1–2 Pixel
- Cloud-Layer ko-rotiert mit ~1.2× Erdgeschwindigkeit (synthetisch — die MODIS-Cloud-Map ist ein statischer Composite, kein Echtzeit-Wetter)
- Mondphasen werden als gefüllter Punkt mit Helligkeit dargestellt, ohne Phasenrichtung (Sichel links vs. rechts)
- Snapshot-Modus rendert immer 80×40 in Blocks-Modus
- Klassifikations-Heuristik (RGB → Klasse) ist robust, aber JPG-Komprimierungsartefakte produzieren gelegentlich Misklassifikationen am Pixel-Rand
