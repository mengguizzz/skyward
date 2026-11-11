"""Verify and unpack the fixed, minimal pipa vendor runtime input."""

import hashlib
import json
from pathlib import Path
import sys
import tarfile


def unpack(destination):
    here = Path(__file__).resolve().parent
    locked = json.loads((here / "inputs.lock.json").read_text())["vendor_runtime"]
    archive = here / "inputs" / "pipa-vendor-runtime.tar.gz"
    content = archive.read_bytes()
    if len(content) != locked["size"] or hashlib.sha256(content).hexdigest() != locked["sha256"]:
        raise SystemExit("Bundled vendor runtime does not match inputs.lock.json")
    if destination.exists():
        raise SystemExit("Runtime output already exists; use a fresh output directory")
    with tarfile.open(archive, "r:gz") as bundle:
        for member in bundle.getmembers():
            path = Path(member.name)
            if path.is_absolute() or ".." in path.parts or not (member.isfile() or member.isdir()):
                raise SystemExit(f"Unsafe runtime archive member: {member.name}")
        bundle.extractall(destination, filter="data")
    manifest = json.loads((destination / "manifest.json").read_text())
    files = manifest["files"]
    if len(files) != locked["file_count"] or sum(file["size"] for file in files) != locked["files_size"]:
        raise SystemExit("Runtime manifest totals do not match inputs.lock.json")
    print("Fixed minimal vendor runtime unpacked; packer verifies each file's hash and mode.")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("Usage: python3 tools/pipa-ci/runtime_input.py OUTPUT_DIR")
    unpack(Path(sys.argv[1]))
