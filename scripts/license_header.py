#!/usr/bin/env python3
# Copyright (C) 2026 The Suhina contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Checks that every tracked source file (.rs, .py, .sh) starts with the licence header.

    python scripts/license_header.py           # add the header where it is missing
    python scripts/license_header.py --check   # list the files without it, change nothing

The header, after a `#!` line when there is one, then an empty line:

    // Copyright (C) 2026 The Suhina contributors
    // SPDX-License-Identifier: GPL-3.0-or-later

(`#` instead of `//` in scripts). The same rule as in Suhina's repository.
"""

from __future__ import annotations

import subprocess
import sys

COPYRIGHT = "Copyright (C) 2026 The Suhina contributors"
SPDX = "SPDX-License-Identifier: GPL-3.0-or-later"
COMMENT = {".rs": "//", ".py": "#", ".sh": "#"}


def tracked() -> list[str]:
    out = subprocess.run(["git", "ls-files"], capture_output=True, text=True, check=True).stdout
    return [path for path in out.splitlines() if any(path.endswith(ext) for ext in COMMENT)]


def has_header(lines: list[str], prefix: str) -> bool:
    start = 1 if lines and lines[0].startswith("#!") else 0
    head = lines[start : start + 2]
    return head == [f"{prefix} {COPYRIGHT}", f"{prefix} {SPDX}"]


def add_header(path: str, lines: list[str], prefix: str) -> None:
    start = 1 if lines and lines[0].startswith("#!") else 0
    header = [f"{prefix} {COPYRIGHT}", f"{prefix} {SPDX}", ""]
    new = lines[:start] + header + lines[start:]
    with open(path, "w", encoding="utf-8", newline="\n") as file:
        file.write("\n".join(new) + "\n")


def main() -> int:
    check = "--check" in sys.argv[1:]
    missing = []
    for path in tracked():
        prefix = COMMENT[path[path.rfind(".") :]]
        with open(path, encoding="utf-8") as file:
            lines = file.read().splitlines()
        if has_header(lines, prefix):
            continue
        missing.append(path)
        if not check:
            add_header(path, lines, prefix)
    for path in missing:
        print(("missing header: " if check else "added header: ") + path)
    return 1 if check and missing else 0


if __name__ == "__main__":
    sys.exit(main())
