"""Independently verify the source-generated boot overlay before packaging."""

import hashlib
import importlib.util
import subprocess
import sys
import tempfile
from pathlib import Path

sys.dont_write_bytecode = True


def verify_routing(repo, base, image, loader, data):
    spec = importlib.util.spec_from_file_location(
        "pipa_verify_image", repo / "experimental/pipa/verify_image.py"
    )
    verify = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(verify)
    check = verify.check
    _, base_kernel, base_rd = verify.boot(base)
    _, current_kernel, current_rd = verify.boot(image)
    check(base_kernel == current_kernel, "routing base kernel changed")
    original, current = verify.entries(base_rd), verify.entries(current_rd)
    overlay = verify.entries((data / "overlay.gz").read_bytes())
    check(original["system/bin/init"] == current["system/bin/init"], "first-stage init changed")
    init = "system/etc/init/hw/init.rc"
    metadata, rules = current[init]
    needle = b"service recovery /system/bin/recovery\n"
    check(rules.count(needle) == 1, "unexpected recovery service")
    current[init] = metadata, rules.replace(
        needle, b"service recovery /system/bin/recovery-loader\n", 1
    )
    current["system/bin/recovery-loader"] = current["system/bin/recovery"][0], loader.read_bytes()
    expected = {name: entry for name, entry in current.items() if original.get(name) != entry}
    stamp = "skyward/boot-overlay.sha256"
    check(set(overlay) == set(expected) | {stamp}, "unexpected generated overlay paths")
    check("system/bin/init" not in overlay, "overlay changes first-stage init")
    check(all(overlay[name] == entry for name, entry in expected.items()), "overlay differs from current build")
    manifest = []
    for name, (metadata, content) in expected.items():
        mode, uid, gid = metadata[:3]
        manifest.append(f"{hashlib.sha256(content).hexdigest()} {mode:o} {uid} {gid} {name}\n")
    check(overlay[stamp][0][:3] == (0o100644, 0, 0), "unexpected stamp metadata")
    check(overlay[stamp][1] == "".join(sorted(manifest)).encode(), "overlay stamp mismatch")
    targets = {}
    for line in (data / "original-targets.txt").read_text().splitlines():
        digest, name = line.split(" ", 1)
        check(name not in targets, "duplicate patch target")
        targets[name] = digest
    check(set(targets) == set(overlay), "patch target set mismatch")
    for name, digest in targets.items():
        expected_hash = hashlib.sha256(original[name][1]).hexdigest() if name in original else "-"
        check(digest == expected_hash, f"unexpected original target hash: {name}")
    return {
        "source_generated": True,
        "entries": sorted(overlay),
        "overlay_sha256": hashlib.sha256((data / "overlay.gz").read_bytes()).hexdigest(),
        "fallback_sha256": hashlib.sha256(current["system/bin/recovery"][1]).hexdigest(),
        "loader_sha256": hashlib.sha256(loader.read_bytes()).hexdigest(),
        "first_stage_init_preserved": True,
        "hardware_tested": False,
    }


def verify_patch(repo, base, data, patcher, out):
    spec = importlib.util.spec_from_file_location(
        "pipa_verify_patch", repo / "experimental/pipa/verify_image.py"
    )
    verify = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(verify)
    original, kernel, ramdisk = verify.boot(base)
    original_entries = verify.entries(ramdisk)
    targets = {line.split(" ", 1)[1] for line in (data / "original-targets.txt").read_text().splitlines()}
    overlay = verify.entries((data / "overlay.gz").read_bytes())
    # Exercise the real host patcher on image files only; no device is accessed.
    with tempfile.TemporaryDirectory(prefix="patch-check-", dir=out) as scratch:
        first, repeated = Path(scratch) / "first.img", Path(scratch) / "repeated.img"
        fixture = Path(scratch) / "normal-boot-fixture.img"
        command_line = original[44:1580].split(b"\0", 1)[0].split()
        forced = any(token.startswith(b"twrpfastboot=") for token in command_line)
        if forced:
            refused = subprocess.run(
                [str(patcher), str(base), str(data), str(first)], capture_output=True, text=True
            )
            verify.check(refused.returncode != 0 and "forced-recovery" in refused.stderr,
                         "patcher did not refuse forced-recovery boot")
        # The standalone image deliberately forces recovery. Model a normal TWRP
        # input by clearing that header argument in a disposable local fixture.
        normal = bytearray(original)
        normal[44:1580] = b" ".join(
            token for token in command_line if not token.startswith(b"twrpfastboot=")
        ).ljust(1536, b"\0")
        fixture.write_bytes(normal)
        subprocess.run([str(patcher), str(fixture), str(data), str(first)], check=True)
        subprocess.run([str(patcher), str(first), str(data), str(repeated)], check=True)
        first_data, first_kernel, first_rd = verify.boot(first)
        repeated_data, repeated_kernel, repeated_rd = verify.boot(repeated)
        installed = verify.entries(first_rd)
        verify.check(first_kernel == repeated_kernel == kernel, "patch changed kernel")
        verify.check(installed == verify.entries(repeated_rd), "repeat installation changed ramdisk")
        verify.check(first_data[44:1580] == repeated_data[44:1580], "repeat installation changed command line")
        for name, entry in original_entries.items():
            if name not in targets:
                verify.check(installed.get(name) == entry, f"patch changed unrelated entry: {name}")
        for name, entry in overlay.items():
            if name not in {"prop.default", "skyward/boot-overlay.sha256"}:
                verify.check(installed.get(name) == entry, f"patch omitted generated entry: {name}")
        verify.avb(first_data)
        verify.avb(repeated_data)
        verify.check(original[16:44] == first_data[16:44], "patch changed boot header settings")
    return {
        "fresh_install": True,
        "repeat_install": True,
        "kernel_and_first_stage_init_preserved": True,
        "unrelated_ramdisk_entries_preserved": True,
        "avb_digest_verified": True,
        "synthetic_normal_boot_fixture": True,
        "forced_recovery_input_refused": forced,
    }
