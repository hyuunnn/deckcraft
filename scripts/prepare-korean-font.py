#!/usr/bin/env python3
"""Add a pinned OFL Korean fallback to a craft-fonts checkout (never to DeckCraft).

Run explicitly before building; build.rs itself stays offline. CI/release workflows use the
same recipe, checksums and licence. Safe to repeat, including without network access.
"""

import hashlib
from pathlib import Path
import sys
import urllib.request

SOURCE = "https://github.com/google/fonts/tree/9710da1eacb3be272583c3224dcb70f9da6eadbb/ofl/gothica1"
RAW = SOURCE.replace("github.com", "raw.githubusercontent.com").replace("/tree/", "/")
FILES = {
    "GothicA1-Regular.ttf": "211151bea98098c579610ab1114bb1bfe909057207db561f357b896d076c21ac",
    "OFL.txt": "ed95c33f80ccca002e3a360b683c43368f9c5eb024e5b992abb51af3c10b59bc",
}


def prepare(root: Path) -> None:
    manifest = root / "fonts/manifest.txt"
    content = manifest.read_text(encoding="utf-8")  # Require an existing craft-fonts checkout.
    attribution = root / "ATTRIBUTION.md"
    credits = attribution.read_text(encoding="utf-8")
    target = root / "fonts/gothic-a1"
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
        "Gothic A1 | Regular | fonts/gothic-a1/GothicA1-Regular.ttf | Hang,Latn | "
        f"OFL-1.1 | fonts/gothic-a1/OFL.txt | {FILES['GothicA1-Regular.ttf']} | {SOURCE}"
    )
    # Replace our entry instead of duplicating it on subsequent runs.
    lines = [
        line for line in content.splitlines()
        if not line.startswith(("Nanum Gothic | Regular |", "NanumGothic | Regular |", "Gothic A1 | Regular |"))
    ]
    manifest.write_text("\n".join([*lines, entry]) + "\n", encoding="utf-8")
    credit = (
        "| `fonts/gothic-a1/GothicA1-Regular.ttf` | Gothic A1 Regular | "
        f"HanYang I&C Co., Ltd. | [{SOURCE}]({SOURCE}) | "
        "OFL-1.1 (`fonts/gothic-a1/OFL.txt`) |"
    )
    credits = "\n".join(line for line in credits.splitlines() if "`fonts/nanum-gothic/NanumGothic-Regular.ttf`" not in line)
    if "`fonts/gothic-a1/GothicA1-Regular.ttf`" not in credits:
        credits = credits.rstrip() + "\n\n" + credit
    attribution.write_text(credits.rstrip() + "\n", encoding="utf-8")
    # Replace the earlier fallback instead of embedding two Korean faces. Only remove files
    # from our previous recipe; preserve any other local additions to that directory.
    legacy = root / "fonts/nanum-gothic"
    for name in ("NanumGothic-Regular.ttf", "OFL.txt"):
        (legacy / name).unlink(missing_ok=True)
    if legacy.is_dir() and not any(legacy.iterdir()):
        legacy.rmdir()
    print(f"Korean fallback ready: {target}")


if __name__ == "__main__":
    try:
        prepare(Path(sys.argv[1] if len(sys.argv) > 1 else "../craft-fonts"))
    except (OSError, ValueError) as error:
        sys.exit(f"Cannot prepare Korean font: {error}")
