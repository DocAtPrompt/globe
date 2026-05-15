#!/usr/bin/env python3
"""Konvertiert NASA-basierte Earth-Texturen in die Binär-Assets, die `globe`
zur Compile-Zeit einbettet.

Source (assets/raw/, müssen vorhanden sein):
    earth_atmos.jpg     RGB Blue-Marble-Day-Map
    earth_lights.png    Black-Marble nightlights
    earth_clouds.png    Cloud-Cover-Composite

Output (assets/):
    earth_classes.bin.z  W·H bytes, zlib-deflate, classes 0..5
    earth_lights.bin.z   W·H bytes, zlib-deflate, brightness 0..255
    earth_clouds.bin.z   W·H bytes, zlib-deflate, alpha 0..255

Plus eine .meta-Header-Datei pro Asset, damit das Rust-Modul die Auflösung
zur Laufzeit kennt.
"""
import struct
import urllib.request
import zlib
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent / "assets"
RAW = ROOT / "raw"

# Source-Texturen — NASA Public-Domain-Material, gehostet über den Three.js
# Texture-Mirror (stabile URLs). Lizenz: Public Domain (NASA).
SOURCE_URLS = {
    "earth_atmos.jpg":    "https://threejs.org/examples/textures/planets/earth_atmos_2048.jpg",
    "earth_specular.jpg": "https://threejs.org/examples/textures/planets/earth_specular_2048.jpg",
    "earth_lights.png":   "https://threejs.org/examples/textures/planets/earth_lights_2048.png",
    "earth_clouds.png":   "https://threejs.org/examples/textures/planets/earth_clouds_1024.png",
}


def ensure_sources() -> None:
    """Lädt fehlende Source-Bilder aus dem Three.js-Texture-Mirror."""
    RAW.mkdir(parents=True, exist_ok=True)
    for fname, url in SOURCE_URLS.items():
        target = RAW / fname
        if target.exists():
            continue
        print(f"  Lade {url} → {fname} …")
        with urllib.request.urlopen(url, timeout=60) as resp:
            target.write_bytes(resp.read())

# Klassen-IDs — synchron mit src/world.rs (Enum Class)
DEEPSEA, SEA, FLATLAND, UPLAND, MOUNTAIN, ICE = range(6)

MAGIC = b"GLBE"
VERSION = 1


def classify(r: int, g: int, b: int, spec: int, lat_deg: float) -> int:
    """RGB + Specular + Lat → Klasse 0..5.

    Specular map ist eine saubere Land/Wasser-Maske (255 = Wasser, 0 = Land).
    Damit ist die Hauptunterscheidung trivial; RGB bestimmt nur noch die
    Land-Sub-Klasse (Flatland/Upland/Mountain/Ice).
    """
    avg = (r + g + b) / 3
    var = max(r, g, b) - min(r, g, b)

    # --- Wasser (Specular hell) ---
    if spec > 128:
        # Blau-Helligkeit als Tiefen-Proxy: dunkle Pixel = Tiefsee
        return SEA if b > 70 or avg > 50 else DEEPSEA

    # --- Land (Specular dunkel) ---
    # Eis: sehr hell, fast neutral, in hohen Breiten (oder Greenland/Antarktis)
    if avg > 200 and var < 40 and abs(lat_deg) > 55:
        return ICE
    if avg > 220 and var < 20:
        return ICE
    # Berg: hellgrau, mittelhoch, niedrige Sättigung
    if avg > 130 and var < 45:
        return MOUNTAIN
    # Wüste / Hochland: r/g hoch, b deutlich niedriger
    if r > b + 25 and r + g > 2 * b and avg > 90:
        return UPLAND
    # Default Land: alles übrige = Vegetation / Tundra / Flachland
    return FLATLAND


def write_asset(out_path: Path, width: int, height: int, raw: bytes) -> tuple[int, int]:
    """Schreibt: MAGIC(4) + VERSION(u8) + W(u16) + H(u16) + zlib-compressed payload."""
    assert len(raw) == width * height, f"raw size mismatch {len(raw)} != {width*height}"
    header = MAGIC + struct.pack("<BHH", VERSION, width, height)
    compressed = zlib.compress(raw, level=9)
    out_path.write_bytes(header + compressed)
    return len(raw), len(compressed)


def build_classes(src: Path, dst: Path, specular: Path) -> None:
    """Klassifikation kombiniert Blue-Marble-RGB mit der Specular-Land/Wasser-Maske."""
    img = Image.open(src).convert("RGB")
    spec_img = Image.open(specular).convert("L")
    if spec_img.size != img.size:
        spec_img = spec_img.resize(img.size, Image.BILINEAR)
    w, h = img.size
    px = img.load()
    sp = spec_img.load()
    out = bytearray(w * h)
    for y in range(h):
        lat = 90.0 - (y + 0.5) / h * 180.0
        for x in range(w):
            r, g, b = px[x, y]
            out[y * w + x] = classify(r, g, b, sp[x, y], lat)
    raw_size, comp_size = write_asset(dst, w, h, bytes(out))
    print(f"  {dst.name}: {w}x{h}, raw {raw_size:,}B → comp {comp_size:,}B "
          f"({100*comp_size/raw_size:.1f}%)")


def build_grayscale(
    src: Path,
    dst: Path,
    threshold: int = 0,
    channel: str = "L",
) -> None:
    """Konvertiert ein Bild zu 8-bit-Grayscale.

    `channel`: 'L' = luminance, 'A' = alpha-Kanal (für Layer mit transparentem
    Hintergrund wie z. B. Wolken).
    """
    img = Image.open(src)
    if channel == "A":
        img = img.convert("RGBA")
        _r, _g, _b, plane = img.split()
    else:
        plane = img.convert("L")
    w, h = plane.size
    raw = list(plane.getdata())
    if threshold > 0:
        scale = 255.0 / (255 - threshold) if threshold < 255 else 0
        raw = [
            0 if v < threshold else min(255, int((v - threshold) * scale))
            for v in raw
        ]
    raw_size, comp_size = write_asset(dst, w, h, bytes(raw))
    print(f"  {dst.name}: {w}x{h}, raw {raw_size:,}B → comp {comp_size:,}B "
          f"({100*comp_size/raw_size:.1f}%)")


def main() -> int:
    ROOT.mkdir(parents=True, exist_ok=True)
    print("Stelle Source-Bilder sicher …")
    ensure_sources()
    # (Quelle, Ziel, Funktion, optionale-Kwargs)
    sources = [
        ("earth_atmos.jpg",  "earth_classes.bin.z", build_classes,
            {"specular": RAW / "earth_specular.jpg"}),
        # Lichter haben in der Three.js-Texture viel diffusen Glow — Threshold
        # filtert das aus, sodass nur echte Stadtlichter übrigbleiben.
        ("earth_lights.png", "earth_lights.bin.z",  build_grayscale,
            {"threshold": 60}),
        # Wolken liegen als RGBA vor — der Alpha-Kanal beschreibt die Bedeckung,
        # nicht die Luminanz. Threshold schneidet Hintergrund-Glow weg.
        ("earth_clouds.png", "earth_clouds.bin.z",  build_grayscale,
            {"channel": "A", "threshold": 30}),
    ]
    print(f"Konvertiere in {ROOT}/ …")
    for src_name, dst_name, fn, kwargs in sources:
        fn(RAW / src_name, ROOT / dst_name, **kwargs)
    print("fertig.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
