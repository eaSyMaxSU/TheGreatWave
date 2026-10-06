"""Package a native release binary, checking it works outside the source tree."""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import zipfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("target", choices=[
        "aarch64-apple-darwin",
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
    ])
    parser.add_argument("tag")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    dist = root / "dist"
    dist.mkdir(exist_ok=True)
    name = f"tgw-{args.tag}-{args.target}"
    executable = "tgw.exe" if args.target.endswith("msvc") else "tgw"

    with tempfile.TemporaryDirectory() as temporary:
        package = Path(temporary) / name
        package.mkdir()
        binary = package / executable
        shutil.copy2(root / "target" / args.target / "release" / executable, binary)
        for document in ("README.md", "LICENSE"):
            shutil.copy2(root / document, package)
        (package / "examples").mkdir()
        for example in ("transfer", "asm", "hls", "gtl"):
            shutil.copy2(root / "examples" / f"{example}.tgw", package / "examples")
        if args.target.endswith("linux-gnu"):
            shutil.copy2(root / "assets/linux/tgw.desktop", package)
            shutil.copy2(root / "assets/icon/tgw.svg", package)

        version = subprocess.check_output([str(binary), "--version"], cwd=package, text=True)
        assert version.strip() == args.tag.removeprefix("v"), version
        help_text = subprocess.check_output([str(binary), "--help"], cwd=package, text=True)
        assert "--view" in help_text and "--watch" in help_text
        assert "This build" not in help_text, "release must include both view and watch"
        for example in ("transfer", "asm", "hls", "gtl"):
            svg = subprocess.check_output(
                [str(binary), f"examples/{example}.tgw"], cwd=package
            )
            assert b"<svg" in svg and b"</svg>" in svg, example

        if os.name == "nt":
            archive = dist / f"{name}.zip"
            with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as output:
                for path in sorted(package.rglob("*")):
                    if path.is_file():
                        output.write(path, path.relative_to(package.parent))
        else:
            archive = dist / f"{name}.tar.gz"
            with tarfile.open(archive, "w:gz") as output:
                output.add(package, arcname=name)
        print(f"Packaged {archive.name}; version, combined features, and all four renders passed")


if __name__ == "__main__":
    main()
