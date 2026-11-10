import hashlib
import json
import pathlib
import shutil
import subprocess
import sys
import zipfile

REPO = pathlib.Path(__file__).resolve().parents[2]
LOCK = json.loads((REPO / "tools/pipa-ci/inputs.lock.json").read_text())
ROUTING_NAMES = {"overlay.gz", "original-targets.txt", "init.sha256", "kernels.sha256"}


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def validate(base, routing):
    if base.stat().st_size != LOCK["base"]["size"] or digest(base) != LOCK["base"]["sha256"]:
        raise SystemExit("Base boot differs from the tested boot-fbboot.img")
    if digest(routing) != LOCK["routing"]["sha256"]:
        raise SystemExit("Frozen boot routing archive hash mismatch")
    with zipfile.ZipFile(routing) as archive:
        if set(archive.namelist()) != ROUTING_NAMES or len(archive.namelist()) != len(ROUTING_NAMES):
            raise SystemExit("Unexpected frozen routing files")
        if archive.testzip() is not None:
            raise SystemExit("Corrupt routing archive")
        for name, expected in LOCK["routing"]["files"].items():
            if hashlib.sha256(archive.read(name)).hexdigest() != expected:
                raise SystemExit(f"Frozen routing file hash mismatch: {name}")
    print("Input images and routing archive validated.")


def package(base, routing, target, out):
    validate(base, routing)
    stage = out / "installer-staging"
    if stage.exists():
        raise SystemExit("installer-staging already exists; use a fresh output directory")
    shutil.copytree(REPO / "tools/pipa-cust-installer/package", stage)
    (stage / "data").mkdir()
    (stage / "tools").mkdir()
    (stage / "payload").mkdir()
    with zipfile.ZipFile(routing) as archive:
        for name in sorted(ROUTING_NAMES):
            (stage / "data" / name).write_bytes(archive.read(name))
    for name in ("boot-patcher", "recovery-loader", "cust-readonly"):
        shutil.copyfile(target / "pipa-cust-installer/aarch64-linux-android/release" / name, stage / "tools" / name)
    binary = target / "aarch64-linux-android/release/skyward"
    shutil.copyfile(binary, stage / "payload/recovery")
    (stage / "payload/manifest.txt").write_text("pipa-v8-userspace-prototype-1\n" + digest(binary) + "\n")
    shutil.copyfile(REPO / "LICENSE", stage / "LICENSE")
    lines = [f"{digest(p)}  {p.relative_to(stage).as_posix()}"
             for p in sorted(stage.rglob("*")) if p.is_file()]
    (stage / "SHA256SUMS").write_text("\n".join(lines) + "\n")
    result = out / "skyward-pipa-cust-installer.zip"
    with zipfile.ZipFile(result, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for path in sorted(stage.rglob("*")):
            if not path.is_file():
                continue
            name = path.relative_to(stage).as_posix()
            info = zipfile.ZipInfo(name, (2026, 11, 10, 0, 0, 0))
            info.create_system = 3
            executable = name.startswith("tools/") or name.endswith(".sh") or name.endswith("update-binary")
            info.external_attr = (0o100755 if executable else 0o100644) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, path.read_bytes(), compresslevel=9)
    with zipfile.ZipFile(result) as archive:
        if archive.testzip() is not None:
            raise SystemExit("Installer ZIP verification failed")
        for line in lines:
            expected, name = line.split("  ", 1)
            if hashlib.sha256(archive.read(name)).hexdigest() != expected:
                raise SystemExit(f"Installer member hash mismatch: {name}")
    try:
        revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip()
    except subprocess.CalledProcessError:
        revision = "unknown"
    (out / "build-info.json").write_text(json.dumps({
        "fork_commit": revision,
        "port_baseline": LOCK["upstream_revision"],
        "inputs": LOCK,
        "payload_sha256": digest(binary),
        "hardware_tested": False,
        "boot_fallback": "historical-v8",
    }, indent=2) + "\n")
    (out / "SHA256SUMS").write_text("\n".join(
        f"{digest(out / name)}  {name}"
        for name in ("skyward-pipa-boot.img", result.name, "verification.json", "build-info.json")
    ) + "\n")
    print(f"Installer ZIP verified: {len(lines)} members plus SHA256SUMS")


if __name__ == "__main__":
    if sys.argv[1] == "validate" and len(sys.argv) == 4:
        validate(*map(pathlib.Path, sys.argv[2:]))
    elif sys.argv[1] == "package" and len(sys.argv) == 6:
        package(*map(pathlib.Path, sys.argv[2:]))
    else:
        raise SystemExit("Usage: package.py validate BASE ROUTING | package BASE ROUTING TARGET OUT")
