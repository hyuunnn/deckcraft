#!/usr/bin/env python3
"""Add a pinned OFL Korean fallback to a craft-fonts checkout (never to DeckCraft).

Run explicitly before building; build.rs itself stays offline. CI/release workflows use the
same recipe, checksums and licence. Safe to repeat, including without network access.
"""

import hashlib
from pathlib import Path
import sys
import urllib.request

SOURCE = "https://github.com/google/fonts/tree/9710da1eacb3be272583c3224dcb70f9da6eadbb/ofl/nanumgothic"
RAW = SOURCE.replace("github.com", "raw.githubusercontent.com").replace("/tree/", "/")
FILES = {
    "NanumGothic-Regular.ttf": "76f45ef4a6bcff344c837c95a7dcc26e017e38b5846d5ae0cdcb5b86be2e2d31",
    "OFL.txt": "eeacf16032901d0ed0456876ec77b8f0fda6b3fecec7d972f8543eb602e6c30f",
}


def prepare(root: Path) -> None:
    manifest = root / "fonts/manifest.txt"
    content = manifest.read_text(encoding="utf-8")  # Require an existing craft-fonts checkout.
    attribution = root / "ATTRIBUTION.md"
    credits = attribution.read_text(encoding="utf-8")
    target = root / "fonts/nanum-gothic"
    payloads = {}
    for name, checksum in FILES.items():
        path = target / name
        data = path.read_bytes() if path.exists() else b""
        if hashlib.sha256(data).hexdigest() != checksum:
            with urllib.request.urlopen(f"{RAW}/{name}", timeout=60) as response:
                data = response.read(8 * 1024 * 1024)
            if hashlib.sha256(data).hexdigest() != checksum:
                raise ValueError(f"{name}: upstream SHA-256 mismatch")
        payloads[name] = data
    # Do not leave an unattributed font behind if either download or verification fails.
    target.mkdir(parents=True, exist_ok=True)
    for name, data in payloads.items():
        (target / name).write_bytes(data)
    entry = (
        "NanumGothic | Regular | fonts/nanum-gothic/NanumGothic-Regular.ttf | Hang,Latn | "
        f"OFL-1.1 | fonts/nanum-gothic/OFL.txt | {FILES['NanumGothic-Regular.ttf']} | {SOURCE}"
    )
    # Replace our entry instead of duplicating it on subsequent runs.
    lines = [line for line in content.splitlines() if not line.startswith(("Nanum Gothic | Regular |", "NanumGothic | Regular |"))]
    manifest.write_text("\n".join([*lines, entry]) + "\n", encoding="utf-8")
    credit = (
        "| `fonts/nanum-gothic/NanumGothic-Regular.ttf` | Nanum Gothic Regular | "
        f"NHN Corporation; Sandoll Communications | [{SOURCE}]({SOURCE}) | "
        "OFL-1.1 (`fonts/nanum-gothic/OFL.txt`) |"
    )
    if "`fonts/nanum-gothic/NanumGothic-Regular.ttf`" not in credits:
        attribution.write_text(credits.rstrip() + "\n\n" + credit + "\n", encoding="utf-8")
    print(f"Korean fallback ready: {target}")


if __name__ == "__main__":
    try:
        prepare(Path(sys.argv[1] if len(sys.argv) > 1 else "../craft-fonts"))
    except (OSError, ValueError) as error:
        sys.exit(f"Cannot prepare Korean font: {error}")
