"""Command-line entry point for sharepoint-cli.

Locates the native Rust binary installed alongside this package by maturin
and execs it with the user's arguments.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path


def find_native_binary() -> str:
    bin_dir = Path(sys.executable).parent
    for name in ("sharepoint", "sharepoint.exe"):
        candidate = bin_dir / name
        if candidate.is_file():
            return str(candidate)

    found = shutil.which("sharepoint")
    if found:
        return found

    raise FileNotFoundError(
        "Could not find the native sharepoint binary. "
        "Please ensure sharepoint-cli is installed correctly."
    )


def main() -> int:
    try:
        native_binary = find_native_binary()
        args = [native_binary] + sys.argv[1:]

        if sys.platform == "win32":
            completed_process = subprocess.run(args)
            return completed_process.returncode
        else:
            os.execv(native_binary, args)
            return 0
    except FileNotFoundError as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
