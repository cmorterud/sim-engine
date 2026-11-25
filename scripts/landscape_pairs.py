#!/usr/bin/env python3
"""Lays pictures from `--landscape` before and after a change side by side.

    scripts/landscape_pairs.py <before dir> <after dir> <out dir>

Each directory holds one folder per map, as `--landscape <dir>/<map>` makes them. Every picture
found in both becomes one PNG in <out dir>/<map>/, the old view on the left and the new on the
right, at half size. It also prints how much each pair differs, as the mean change per colour
channel out of 255, so that a change that was meant to touch only some views can be checked.
Needs Pillow.
"""

import sys
from pathlib import Path

from PIL import Image, ImageChops, ImageStat

GAP = 8


def main() -> int:
    if len(sys.argv) != 4:
        print(__doc__)
        return 2
    before, after, out = (Path(arg) for arg in sys.argv[1:])
    pairs = 0
    for old in sorted(before.glob("*/*.bmp")):
        new = after / old.parent.name / old.name
        if not new.exists():
            print(f"{old.parent.name}/{old.name}: no picture after")
            continue
        left, right = Image.open(old).convert("RGB"), Image.open(new).convert("RGB")
        change = ImageStat.Stat(ImageChops.difference(left, right.resize(left.size))).mean
        print(f"{old.parent.name}/{old.stem}: mean change {sum(change) / 3:.2f}")
        width, height = left.size
        pair = Image.new("RGB", (width * 2 + GAP, height), "white")
        pair.paste(left, (0, 0))
        pair.paste(right.resize(left.size), (width + GAP, 0))
        target = out / old.parent.name / f"{old.stem}.png"
        target.parent.mkdir(parents=True, exist_ok=True)
        pair.resize((pair.width // 2, pair.height // 2), Image.LANCZOS).save(target)
        pairs += 1
    print(f"{pairs} pairs in {out}")
    return 0 if pairs else 1


if __name__ == "__main__":
    sys.exit(main())
