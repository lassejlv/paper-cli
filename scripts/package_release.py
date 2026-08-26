#!/usr/bin/env python3

import argparse
import tarfile
import zipfile
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Package a compiled paper binary for a GitHub release."
    )
    parser.add_argument("--target", required=True, help="Rust target triple")
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--format", required=True, choices=("tar.gz", "zip"))
    parser.add_argument("--output", required=True, type=Path)
    return parser.parse_args()


def required_file(path: Path) -> Path:
    if not path.is_file():
        raise SystemExit(f"required release file does not exist: {path}")
    return path


def package_tar(output: Path, files: list[tuple[Path, str]]) -> None:
    with tarfile.open(output, "w:gz") as archive:
        for source, archive_name in files:
            archive.add(source, arcname=archive_name)


def package_zip(output: Path, files: list[tuple[Path, str]]) -> None:
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for source, archive_name in files:
            archive.write(source, arcname=archive_name)


def main() -> None:
    args = parse_args()
    binary = required_file(args.binary)
    license_file = required_file(Path("LICENSE"))
    readme = required_file(Path("README.md"))
    binary_name = "paper.exe" if binary.suffix.lower() == ".exe" else "paper"
    files = [
        (binary, binary_name),
        (license_file, "LICENSE"),
        (readme, "README.md"),
    ]

    args.output.parent.mkdir(parents=True, exist_ok=True)
    if args.format == "tar.gz":
        package_tar(args.output, files)
    else:
        package_zip(args.output, files)

    print(f"packaged {args.target}: {args.output}")


if __name__ == "__main__":
    main()
