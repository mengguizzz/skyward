#!/usr/bin/env python3
"""Check a recovery-as-boot image without writing or flashing a device."""

import argparse
import gzip
import hashlib
import json
import struct
import tomllib
from pathlib import Path


def check(condition, message):
    if not condition:
        raise ValueError(message)


def boot(path):
    data = path.read_bytes()
    check(data[:8] == b"ANDROID!", "not an Android boot image")
    kernel_size, ramdisk_size = struct.unpack_from("<II", data, 8)
    check(struct.unpack_from("<I", data, 40)[0] == 3, "expected header v3")
    ramdisk_offset = 4096 + ((kernel_size + 4095) // 4096) * 4096
    kernel = data[4096:4096 + kernel_size]
    ramdisk = data[ramdisk_offset:ramdisk_offset + ramdisk_size]
    check(len(kernel) == kernel_size and len(ramdisk) == ramdisk_size, "truncated image")
    return data, kernel, ramdisk


def entries(compressed):
    archive = gzip.decompress(compressed)
    result = {}
    offset = 0
    while True:
        check(archive[offset:offset + 6] == b"070701", "invalid newc header")
        fields = [int(archive[offset + 6 + i * 8:offset + 14 + i * 8], 16) for i in range(13)]
        name_start = offset + 110
        name_end = name_start + fields[11]
        check(archive[name_end - 1:name_end] == b"\0", "unterminated cpio name")
        name = archive[name_start:name_end - 1].decode()
        start = (name_end + 3) & ~3
        end = start + fields[6]
        check(end <= len(archive), "truncated cpio entry")
        if name == "TRAILER!!!":
            break
        check(name not in result, f"duplicate cpio entry: {name}")
        # Ignore inode numbering, which changes when an archive is rebuilt.
        result[name] = (tuple(fields[1:6] + fields[7:11]), archive[start:end])
        offset = (end + 3) & ~3
    return result


def avb(data):
    magic, major, minor, image_size, vbmeta_offset, vbmeta_size = struct.unpack_from(">4sIIQQQ", data, len(data) - 64)
    check(magic == b"AVBf" and major == 1, "invalid AVB footer")
    check(image_size <= vbmeta_offset, "AVB metadata overlaps image")
    check(vbmeta_offset + vbmeta_size <= len(data) - 64, "AVB metadata exceeds image")
    vbmeta = data[vbmeta_offset:vbmeta_offset + vbmeta_size]
    check(vbmeta[:4] == b"AVB0", "missing vbmeta")
    auth_size = struct.unpack_from(">Q", vbmeta, 12)[0]
    check(struct.unpack_from(">I", vbmeta, 28)[0] == 0, "expected unsigned experimental vbmeta")
    desc_offset, desc_size = struct.unpack_from(">QQ", vbmeta, 96)
    start = 256 + auth_size + desc_offset
    desc = vbmeta[start:start + desc_size]
    check(struct.unpack_from(">Q", desc)[0] == 2, "expected hash descriptor")
    hashed_size = struct.unpack_from(">Q", desc, 16)[0]
    check(hashed_size == image_size, "hash size differs from AVB image size")
    check(desc[24:56].rstrip(b"\0") == b"sha256", "unexpected hash algorithm")
    name_size, salt_size, digest_size = struct.unpack_from(">III", desc, 56)
    name = desc[132:132 + name_size].decode()
    salt_start = 132 + name_size
    salt = desc[salt_start:salt_start + salt_size]
    digest = desc[salt_start + salt_size:salt_start + salt_size + digest_size]
    check(name == "boot", f"AVB partition is {name}, expected boot")
    check(hashlib.sha256(salt + data[:image_size]).digest() == digest, "AVB image digest mismatch")
    return {"partition": name, "hashed_bytes": hashed_size, "digest_verified": True, "signed": False}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("base", type=Path)
    parser.add_argument("image", type=Path)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--kernel-arg")
    parser.add_argument("--functionfs-mtp", action="store_true")
    parser.add_argument("--vendor-runtime", type=Path)
    parser.add_argument("--vendor-launcher", type=Path)
    parser.add_argument("--stat-compat", type=Path)
    parser.add_argument("--reinstall-zip", type=Path)
    args = parser.parse_args()
    source, source_kernel, source_rd = boot(args.base)
    image, kernel, ramdisk = boot(args.image)
    check(len(image) == len(source) == 192 * 1024 * 1024, "expected a 192 MiB partition image")
    check(source_kernel == kernel, "base kernel changed")
    check(source[16:44] == image[16:44], "boot header settings changed")
    expected_cmdline = source[44:1580]
    if args.kernel_arg:
        text = expected_cmdline.split(b"\0", 1)[0]
        text = text + (b" " if text else b"") + args.kernel_arg.encode("ascii")
        check(len(text) < 1536, "command line too long")
        expected_cmdline = text.ljust(1536, b"\0")
    check(expected_cmdline == image[44:1580], "unexpected kernel command line change")
    old, new = entries(source_rd), entries(ramdisk)
    if args.reinstall_zip:
        archive = args.reinstall_zip.read_bytes()
        check(new["skyward/reinstall/installer.zip"][1] == archive, "reinstall ZIP mismatch")
        check(new["skyward/reinstall/installer.sha256"][1] ==
              (hashlib.sha256(archive).hexdigest() + "\n").encode(), "reinstall digest mismatch")
        for path in ("skyward/reinstall/installer.zip", "skyward/reinstall/installer.sha256"):
            check(new[path][0][:3] == (0o100644, 0, 0), "reinstall resource metadata mismatch")
    allowed = {
        "system/bin/recovery", "system/bin/7za", "system/bin/avbctl",
        "system/bin/resetprop", "system/bin/unzip", "system/bin/zstd",
        "prop.default",
    }
    if args.functionfs_mtp:
        allowed.add("init.recovery.usb.rc")
    if args.vendor_runtime:
        allowed.add("init.recovery.qcom.rc")
        allowed.add("system/etc/init/hw/init.rc")
    changed = sorted(name for name in old if new.get(name) != old[name])
    check(set(changed) <= allowed, f"unexpected ramdisk changes: {set(changed) - allowed}")
    check(set(old) <= set(new), "original ramdisk entries removed")
    if args.vendor_runtime:
        check(args.vendor_launcher is not None, "runtime launcher missing")
        launcher = args.vendor_launcher.read_bytes()
        check(new["system/bin/pipa-vendor-exec"][1] == launcher, "runtime launcher mismatch")
        check(args.stat_compat is not None, "stat compatibility library missing")
        compat = args.stat_compat.read_bytes()
        metadata, contents = new["system/lib64/libskyward-stat-compat.so"]
        check(contents == compat and metadata[:3] == (0o100644, 0, 0), "stat compatibility mismatch")
        check(compat[:5] == b"\x7fELF\x02" and struct.unpack_from("<HH", compat, 16) == (3, 183),
              "stat compatibility library is not AArch64 ET_DYN")
        check(launcher[:5] == b"\x7fELF\x02"
              and struct.unpack_from("<H", launcher, 18)[0] == 183,
              "runtime launcher is not AArch64 ELF64")
        manifest_bytes = (args.vendor_runtime / "manifest.json").read_bytes()
        check(new["skyward/vendor-runtime/manifest.json"][1] == manifest_bytes,
              "runtime manifest mismatch")
        manifest = json.loads(manifest_bytes)
        check(manifest["format"] == "pipa-vendor-runtime-v1", "runtime format mismatch")
        expected_names = set()
        for file in manifest["files"]:
            name = "skyward/vendor-runtime/" + file["path"]
            check(name not in expected_names, "duplicate runtime member")
            expected_names.add(name)
            metadata, content = new[name]
            check(len(content) == file["size"]
                  and hashlib.sha256(content).hexdigest() == file["sha256"],
                  f"runtime content mismatch: {name}")
            check(metadata[:3] == (0o100000 | file["mode"], 0, 0),
                  f"runtime ownership/mode mismatch: {name}")
        extras = {name for name, (metadata, _) in new.items()
                  if name.startswith("skyward/vendor-runtime/")
                  and metadata[0] & 0o170000 != 0o040000}
        check(extras == expected_names | {
            "skyward/vendor-runtime/manifest.json", "skyward/vendor-runtime/firmware_mnt"
        }, "unlisted runtime files")
        check(new["skyward/vendor-runtime/firmware_mnt"][1] == b"/firmware",
              "runtime modem path mismatch")
        expected_rules = old["init.recovery.qcom.rc"][1]
        libraries = b"    export LD_LIBRARY_PATH /system/lib64:/vendor/lib64:/vendor/lib64/hw\n"
        check(expected_rules.count(libraries) == 1, "unexpected QCOM library environment")
        expected_rules = expected_rules.replace(libraries,
            b"    export LD_LIBRARY_PATH /system/lib64\n    export LD_PRELOAD /system/lib64/libskyward-stat-compat.so\n", 1)
        trigger = b"on property:vendor.sys.listeners.registered=true\n"
        check(expected_rules.count(trigger) == 1, "unexpected secure service trigger")
        expected_rules = expected_rules.replace(trigger,
            b"on property:vendor.sys.listeners.registered=true && property:skyward.vendor-runtime.ready=1\n", 1)
        trigger = b"on init\n    start vendor.qseecomd\n"
        check(expected_rules.count(trigger) == 1, "unexpected qsee trigger")
        expected_rules = expected_rules.replace(trigger,
            b"on property:skyward.vendor-runtime.ready=1\n    start vendor.qseecomd\n", 1)
        service = b"service vendor.qseecomd /vendor/bin/qseecomd\n"
        check(expected_rules.count(service) == 1, "unexpected qsee service")
        expected_rules = expected_rules.replace(service,
            b"service vendor.qseecomd /system/bin/pipa-vendor-exec --ramdisk-security /vendor/bin/qseecomd\n", 1)
        for service, program in [
            ("keymaster-4-0", "/vendor/bin/hw/android.hardware.keymaster@4.0-service-qti"),
            ("gatekeeper-1-0", "/vendor/bin/hw/android.hardware.gatekeeper@1.0-service-qti"),
        ]:
            original = f"service {service} {program}\n".encode()
            check(expected_rules.count(original) == 1, "unexpected original service")
            expected_rules = expected_rules.replace(original,
                f"service {service} /system/bin/pipa-vendor-exec {program}\n".encode(), 1)
        check(new["init.recovery.qcom.rc"][1] == expected_rules,
              "unexpected init vendor runtime changes")
        libraries = b"    export LD_LIBRARY_PATH /system/lib64:/vendor/lib64/hw\n"
        expected_rules = old["system/etc/init/hw/init.rc"][1]
        check(expected_rules.count(libraries) == 1, "unexpected core library environment")
        expected_rules = expected_rules.replace(libraries, b"    export LD_LIBRARY_PATH /system/lib64\n", 1)
        check(new["system/etc/init/hw/init.rc"][1] == expected_rules, "unexpected core init changes")
    check(new["system/bin/recovery"][1] == args.binary.read_bytes(), "recovery binary mismatch")
    binary = new["system/bin/recovery"][1]
    check(binary[:5] == b"\x7fELF\x02" and struct.unpack_from("<H", binary, 18)[0] == 183, "recovery is not AArch64 ELF64")
    for path in ("system/bin/linker64", "system/lib64/libdl.so", "system/lib64/libm.so", "system/lib64/libc.so"):
        check(path in new, f"missing Android runtime file: {path}")
    check(new["skyward/device.toml"][1] == args.config.read_bytes(), "device config mismatch")
    profile = tomllib.loads(args.config.read_text(encoding="utf-8"))
    if args.functionfs_mtp:
        usb_rules = new["init.recovery.usb.rc"][1].decode()
        mtp_rules = new["system/etc/init/skyward-mtp.rc"][1].decode()
        check("mtp.gs0" not in usb_rules, "legacy MTP function still configured")
        check("setprop sys.usb.ffs.mtp.ready 0" not in usb_rules, "MTP readiness still cleared by none")
        check("on property:sys.usb.configfs=1" in mtp_rules, "late FunctionFS setup missing")
        check("sys.usb.config_disable" not in mtp_rules, "ADB start still requires an unset property")
        for value in ("adb", "sideload", "fastboot"):
            check(f"property:sys.usb.config={value}" in usb_rules, f"missing original {value} action")
    binder_header = profile["device"].get("binder_header", "RECO")
    if "binder_header" in profile["device"]:
        expected = f"ro.skyward.binder.header={binder_header}".encode()
        check(expected in new["prop.default"][1].splitlines(), "Binder wire header property missing")
    check(b"service recovery /system/bin/recovery" in new["system/etc/init/hw/init.rc"][1], "recovery init service missing")
    for command in ("7za", "avbctl", "resetprop", "unzip", "zstd"):
        metadata, contents = new[f"system/bin/{command}"]
        check(metadata[0] & 0o170000 == 0o120000 and contents == b"recovery", f"incorrect {command} link")
    for path in ("skyward/MiSansVF.ttf", "skyward/KirkMono-Regular.ttf", "skyward/system-init"):
        check(bool(new[path][1]), f"missing {path}")
    report = {
        "base": str(args.base),
        "base_sha256": hashlib.sha256(source).hexdigest(),
        "image": str(args.image),
        "image_sha256": hashlib.sha256(image).hexdigest(),
        "size_bytes": len(image),
        "kernel_preserved": True,
        "header_settings_preserved": True,
        "appended_kernel_argument": args.kernel_arg,
        "gzip_ramdisk": True,
        "aarch64_recovery": True,
        "android_runtime_files_present": True,
        "binder_header": binder_header,
        "display_workarounds": {
            name: profile.get("display", {}).get(name)
            for name in ("max_refresh_hz", "disable_blanking", "reset_on_open")
        },
        "wlan_configured": "wlan" in profile,
        "functionfs_mtp": args.functionfs_mtp,
        "original_entries": len(old),
        "result_entries": len(new),
        "changed_original_entries": changed,
        "added_entries": sorted(set(new) - set(old)),
        "avb": avb(image),
        "hardware_tested": False,
    }
    args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
