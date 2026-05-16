# globe

Ein rotierender Globus für dein Terminal — mit echtem Sonnenstand, Tag/Nacht-Grenze in Echtzeit, Stadtlichtern auf der Nachtseite, Wolken-Overlay, Sterne, Sonne und Mond, und optionalen Hilfslinien für Äquator und Greenwich-Meridian.

![globe rendered in a terminal: Asia at noon with city lights, atmospheric glow, moon in the night sky](docs/screenshot.png)

```
cargo run --release
```

`q` oder `Esc` zum Beenden.

## Standardansicht

- Kamera auf der **Home-Position** (User-Standort) — beim ersten Start aus der System-Timezone geschätzt; mit `-h LAT,LON` überschreibbar.
- **Auto-Rotation** in Echtzeit (15°/h, eine Umdrehung pro 24 h)
- Echter Sonnenstand für Datum/Uhrzeit; Tag/Nacht-Grenze wandert sichtbar mit der Erdrotation
- Halbblock-Rendering (`▀`) mit 256 ANSI-Farben — läuft in praktisch jedem modernen Terminal
- Aktuelle Mondphase in der Status-Zeile (z.B. "moon: Halbmond 51%")

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
| `,` / `.`          | Rotations-Speed −/+                        | —                                              |
| `m`                | Modus zyklisch: blocks → ascii → plain     | Modus zyklisch                                 |
| `c`                | Wolken-Layer ein/aus                       | Wolken-Layer ein/aus                           |
| `e`                | **Äquator**-Linie (gelb) ein/aus           | Äquator-Linie ein/aus                          |
| `g`                | **Greenwich-Meridian** (blau) ein/aus      | Greenwich-Meridian ein/aus                     |
| `(` / `)`          | Cell-Aspect kalibrieren ±0.05              | Cell-Aspect kalibrieren                        |
| `r`                | Defaults zurück (Position bleibt)          | Defaults + Delta auf null                      |
| `?`                | Hilfe-Overlay                              | Hilfe-Overlay                                  |
| `q` / `Esc`        | Beenden                                    | Beenden                                        |

## CLI

```
globe [-h LAT,LON] [--fps N] [--mode blocks|ascii|plain] [--no-color]
      [--cell-aspect F] [--snapshot]
```

- `-h 48.21,16.37` / `--home 48.21,16.37` — Position als Lat/Lon in Grad (überschreibt Timezone-Schätzung)
- `--fps 30` — Frame-Rate-Limit (1–120)
- `--mode plain` oder `--no-color` — Fallback ohne ANSI-Farben
- `--cell-aspect 2.10` — Verhältnis Cell-Höhe / Cell-Breite, falls die Sphere vertikal gestreckt erscheint (Default 2.0; SF Mono ≈ 2.05, Menlo ≈ 2.10)
- `--snapshot` — ein Frame in stdout schreiben und beenden (für Tests / CI). Nutzt die aktuelle Terminal-Größe.

## Render-Modi

1. **blocks** (Default) — `▀`-Halbblöcke mit zwei Farben pro Zelle → höchste effektive Auflösung
2. **ascii** — ASCII-Helligkeitsrampe `" .'.:;,-~!=+*o#%&@"` mit Farbe je Klasse, Gamma-gespreizt + Klassen-Albedo (Kontinent-Konturen sichtbar)
3. **plain** — ASCII ohne Farben (für Pipes, log-Dateien, restriktive Terminals)

## Visuelle Details

- **Tag/Nacht-Übergang** via smoothstep + Lambert-Shading; Klassenfarben (Tiefsee, Sea, Flatland, Upland, Mountain, Ice) werden zwischen Tag- und Nacht-Palette interpoliert.
- **Stadtlichter** auf der Nachtseite. Bei niedrigem Zoom nur Mega-Cities; beim Reinzoomen werden mittlere und kleine Lichter sichtbar (LOD).
- **Wolken** als zweite Sphere bei Radius 1.005, alpha-gemischt, ko-rotiert mit ~1.2× Erdgeschwindigkeit. Per `c` ausblendbar.
- **Atmosphären-Halo** am Sphere-Rand. Tag-Seite warm gelb-weiß, Sonnenuntergang orange, Dämmerung rötlich-violett, Nacht blau — gemessen am Tangentialpunkt des Strahls zur Sphere.
- **Sterne** im Hintergrund (drei Helligkeitsstufen).
- **Sonne** als 5-zelliger Strahlen-Stern (`*●*` mit oberen/unteren Strahlen).
- **Mond** als 3-zelliger Marker mit Phasen-Symbol (`◐`, `●`, etc.). Distance auf 6 Erdradien komprimiert (real ~60), damit er bei normalem Zoom sichtbar bleibt.
- **Äquator-Linie** (gelb, weich) und **Greenwich-Meridian** (blau, weich) sind toggelbar (`e`/`g`) — beide bleiben unter Erdrotation auf der korrekten geographischen Position.

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
├── vec3.rs          3D-Vektor-Helper + Lat/Lon-Konvertierung + rotate_y
└── world.rs         Eingebettete NASA-Maps (Klassen, Lights, Clouds)
```

## Tests

```
cargo test --lib
```

99 Unit- und Property-Tests in 11 Modulen (u.a. Voll-/Neumond-Termine 2026 trifft das Tool auf den Tag genau, Sahara wird als Land klassifiziert, Pazifik als Tiefsee, Tokio hat Stadtlichter).

## Astronomische Genauigkeit

- **Subsolar-Punkt:** ≈ 0.01° (vereinfachte NOAA-Formel)
- **Mondbahn:** ≈ 1° in Lat/Lon (vereinfachte Meeus-Formel). Mond-Distance ist für die Darstellung auf 6 Erdradien komprimiert; Phasen-Berechnung erfolgt aus der echten Sonne-Mond-Erde-Geometrie.
- **Erdrotation:** akkumuliert aus Wall-Clock; Sonne wird intern in den Welt-Frame rotiert, damit Tag/Nacht-Grenze mit der echten Geschwindigkeit (15°/h) wandert.

## Asset-Pipeline (echte NASA-Maps)

Drei Binär-Dateien in `assets/` werden via `include_bytes!` direkt ins Release-Binary gebacken:

| Datei                  | Auflösung   | Quelle                                                          | Komprimiert |
| ---------------------- | ----------- | --------------------------------------------------------------- | ----------- |
| `earth_classes.bin.z`  | 2048×1024   | Blue Marble Day-Map + Specular-Maske (RGB + Land/Wasser-Trennung) | ~92 KB      |
| `earth_lights.bin.z`   | 2048×1024   | Black Marble Stadtlichter (Threshold + 8-bit)                   | ~52 KB      |
| `earth_clouds.bin.z`   | 1024×512    | MODIS Cloud Cover (Alpha-Channel, 8-bit)                        | ~146 KB     |

Lizenz: alle vier Source-Texturen sind **NASA Public Domain**, gespiegelt im Three.js-Texture-Repository. Konvertiert mit `tools/build_assets.py` (Python + Pillow):

```
python3 tools/build_assets.py
```

Lädt fehlende Source-Bilder per HTTPS, klassifiziert die Day-Map über die Specular-Land/Wasser-Maske + RGB-Heuristik (6 Klassen), filtert die Lights und schreibt das Custom-Container-Format (Magic `GLBE` + Version + Maße + zlib-Payload). Für höher aufgelöste Maps (5400×2700, 8192×4096) tausche die URLs im Script — der Rest des Codes bleibt unverändert.

## Bekannte Begrenzungen

- Map-Auflösung 2048×1024 (~20 km/pixel) — größere Inseln (Sizilien, Sri Lanka, Madagaskar) klar erkennbar; sehr kleine (Malta, Mallorca) nur als 1–2 Pixel
- Cloud-Layer ko-rotiert mit ~1.2× Erdgeschwindigkeit (synthetisch — die MODIS-Map ist ein statischer Composite, kein Echtzeit-Wetter)
- Mondphasen werden als gefüllter Punkt mit Helligkeit dargestellt, ohne Phasenrichtung (Sichel links vs. rechts)
- Klassifikations-Heuristik (RGB → Klasse) ist robust, aber JPG-Komprimierungsartefakte produzieren gelegentlich Misklassifikationen am Pixel-Rand
- Mond-Distance ist artistisch auf 6 Erdradien komprimiert; in echter Geometrie wäre er erst bei extremem Zoom-Out im FOV

## Lizenz und Quellen

Der Rust-Code steht unter der **MIT-Lizenz** (siehe [`LICENSE`](LICENSE)).

Die eingebetteten Map-Assets stammen aus NASA-Quellen (Blue Marble, Black Marble, MODIS Cloud Cover) und sind **Public Domain**. Genaue Quellen-URLs und Attributionen finden sich in [`NOTICE`](NOTICE).

## CI

`cargo build --release`, `cargo test --lib`, `cargo clippy -- -D warnings` werden bei jedem Push und PR auf Linux und macOS automatisch ausgeführt — siehe [`.github/workflows/ci.yml`](.github/workflows/ci.yml).
