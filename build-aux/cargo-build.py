#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Amberol contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Run Cargo and copy its platform-specific executable to Meson's output."""

from pathlib import Path
import shutil
import subprocess
import sys


def main() -> int:
    cargo, built_executable, output, *cargo_options = sys.argv[1:]
    result = subprocess.run([cargo, "build", *cargo_options], check=False)
    if result.returncode != 0:
        return result.returncode
    shutil.copy2(Path(built_executable), Path(output))
    return 0


if __name__ == "__main__":
    sys.exit(main())
