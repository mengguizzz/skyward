"""Exercise the seed installer patcher on image files, never device partitions."""

import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import zipfile

sys.dont_write_bytecode = True


def verify(out, patcher, base):
    repo = Path(__file__).resolve().parents[2]
    spec = importlib.util.spec_from_file_location("retention_image", repo / "experimental/pipa/verify_image.py")
    image = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(image)
    seed = out / "reinstall-seed"
    archive = seed / "skyward-pipa-cust-installer.zip"
    original, _, ramdisk = image.boot(out / "skyward-pipa-boot.img")
    resources = image.entries(ramdisk)
    image.check(resources["skyward/reinstall/installer.zip"][1] == archive.read_bytes(),
                "outer boot does not embed seed ZIP")
    with zipfile.ZipFile(archive) as package:
        inner = image.entries(package.read("data/overlay.gz"))
        image.check(not any(name.startswith("skyward/reinstall") for name in inner),
                    "seed installer recursively contains an installer")
        image.check(b"--retain" in package.read("install.sh"), "retention mode missing")
    with tempfile.TemporaryDirectory(prefix="retain-check-", dir=out) as scratch:
        root = Path(scratch)
        # Model a valid normal-boot header and a kernel package changing kernel
        # bytes. The synthetic kernel is only used for preservation checks.
        normal = bytearray(image.boot(base)[0])
        arguments = normal[44:1580].split(b"\0", 1)[0].split()
        normal[44:1580] = b" ".join(
            arg for arg in arguments if not arg.startswith(b"twrpfastboot=")
        ).ljust(1536, b"\0")
        initial = root / "normal-base.img"
        installed = root / "cust-installed.img"
        initial.write_bytes(normal)
        # Retention repairs a cust installation, whose recovery service and
        # managed manifest differ from the standalone fastboot-boot image.
        subprocess.run([str(patcher), str(initial), str(out / "routing-data"),
                        str(installed)], check=True)
        normal = bytearray(installed.read_bytes())
        normal[4096 + 100] ^= 1
        before = root / "post-kernel.img"
        first = root / "retained.img"
        repeated = root / "repeated.img"
        before.write_bytes(normal)
        for source, target in [(before, first), (first, repeated)]:
            subprocess.run([str(patcher), str(source), str(seed / "routing-data"),
                            str(target), str(archive)], check=True)
        _, wanted_kernel, wanted_ramdisk = image.boot(before)
        written, kernel, ramdisk = image.boot(first)
        _, repeat_kernel, repeat_ramdisk = image.boot(repeated)
        entries = image.entries(ramdisk)
        image.check(kernel == repeat_kernel == wanted_kernel, "new kernel was overwritten")
        image.check(entries == image.entries(repeat_ramdisk), "retention not idempotent")
        image.check(entries == image.entries(wanted_ramdisk), "retention changed managed ramdisk unexpectedly")
        image.check(written[16:1580] == normal[16:1580], "normal boot header or arguments changed")
        image.avb(written)
        # Corruption must be rejected without creating an output or modifying
        # the input image. Full unsupported layouts remain patcher errors.
        bad = root / "unsupported.img"
        bad.write_bytes(b"unsupported boot")
        refused = root / "must-not-exist.img"
        result = subprocess.run([str(patcher), str(bad), str(seed / "routing-data"),
                                 str(refused), str(archive)], capture_output=True)
        image.check(result.returncode != 0 and not refused.exists(), "unsupported boot was written")
        image.check(bad.read_bytes() == b"unsupported boot", "unsupported input was modified")
    report = {
        "seed_zip_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
        "non_recursive": True,
        "new_kernel_preserved": True,
        "normal_boot_header_preserved": True,
        "managed_ramdisk_preserved": True,
        "repeat_install": True,
        "unsupported_input_refused": True,
        "hardware_tested": False,
    }
    (out / "retention-verification.json").write_text(json.dumps(report, indent=2) + "\n")
    with (out / "SHA256SUMS").open("a") as checksums:
        digest = hashlib.sha256((out / "retention-verification.json").read_bytes()).hexdigest()
        checksums.write(f"{digest}  retention-verification.json\n")
    print("Offline RAM installer retention checks passed.")


if __name__ == "__main__":
    verify(Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3]))
