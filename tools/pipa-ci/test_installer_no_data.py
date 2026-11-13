"""Run the real installer flow against disposable files and mocked device tools."""

import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[2]
INSTALLER = REPO / "tools/pipa-cust-installer/package/install.sh"

MOCK = r'''#!/usr/bin/python3
import os
from pathlib import Path
import shutil
import sys

name = Path(sys.argv[0]).name
args = sys.argv[1:]
root = Path(os.environ["INSTALLER_FIXTURE"]).resolve()
store = root / "cust-store"

def local(value):
    path = Path(value).resolve()
    assert path.is_relative_to(root), path
    return path

def sync_tree(source, target):
    source, target = local(source), local(target)
    for child in target.iterdir():
        if child.is_dir():
            shutil.rmtree(child)
        else:
            child.unlink()
    shutil.copytree(source, target, dirs_exist_ok=True)

if name == "id":
    print("0")
elif name == "getprop":
    print({"ro.product.device": "pipa", "ro.boot.slot_suffix": "_b"}.get(args[0], ""))
elif name == "getenforce":
    print("Permissive")
elif name == "blockdev":
    print(201326592 if "boot" in args[-1] else 1073741824)
elif name in ("chown", "sync", "cust-readonly"):
    pass
elif name == "wc":
    # Device geometry is tested separately; small fixtures exercise control flow.
    print(201326592)
elif name == "mount":
    if args[0] != "-o":
        sync_tree(store, args[-1])
elif name == "umount":
    sync_tree(args[0], store)
elif name == "dd":
    values = dict(arg.split("=", 1) for arg in args if "=" in arg)
    source, target = local(values["if"]), local(values["of"])
    if target.name == "boot_b.img" and os.environ.get("INSTALLER_FAIL") == "boot":
        marker = root / "boot-write-failed"
        if not marker.exists():
            marker.touch()
            target.write_bytes(b"partial")
            sys.exit(1)
    shutil.copyfile(source, target)
elif name == "boot-patcher":
    local(args[2]).write_bytes(local(args[0]).read_bytes() + b"-patched")
elif name == "recovery-loader":
    payload = local(args[1])
    if ".prototype-v1-installing" in str(payload) and os.environ.get("INSTALLER_FAIL") == "payload":
        sys.exit(1)
    assert (payload / "recovery").is_file()
    assert (payload / "manifest.txt").is_file()
else:
    raise AssertionError(name)
'''


class InstallerTests(unittest.TestCase):
    def run_install(self, existing=False, failure="", retain=False):
        with tempfile.TemporaryDirectory(prefix="skyward-no-data-") as temp:
            root = Path(temp)
            package = root / "package"
            tools = package / "tools"
            binaries = root / "bin"
            scratch = root / "scratch"
            store = root / "cust-store"
            for folder in (tools, binaries, scratch, store, package / "payload", package / "data"):
                folder.mkdir(parents=True, exist_ok=True)
            original = b"original-boot"
            (root / "boot_b.img").write_bytes(original)
            (root / "cust.img").touch()
            (root / "mounts").touch()
            (store / "unrelated.txt").write_bytes(b"retain")
            if existing:
                (store / "skyward/prototype-v1").mkdir(parents=True)
                (store / "skyward/prototype-v1/recovery").write_bytes(b"new" if retain else b"old")
                (store / "skyward/prototype-v1/manifest.txt").write_bytes(b"new-manifest" if retain else b"old-manifest")
            for name in ("id", "getprop", "getenforce", "blockdev", "chown",
                         "sync", "wc", "mount", "umount", "dd"):
                path = binaries / name
                path.write_text(MOCK)
                path.chmod(0o755)
            for name in ("boot-patcher", "recovery-loader", "cust-readonly"):
                path = tools / name
                path.write_text(MOCK)
                path.chmod(0o755)
            (package / "payload/recovery").write_bytes(b"new")
            (package / "payload/manifest.txt").write_bytes(b"new-manifest")
            script = INSTALLER.read_text()
            replacements = {
                "export PATH=/sbin:/system/bin:/system/xbin:/vendor/bin":
                    f"export PATH={binaries}:/usr/bin:/bin",
                'boot="/dev/block/bootdevice/by-name/boot$slot"':
                    f'boot="{root}/boot${{slot}}.img"',
                "cust=/dev/block/bootdevice/by-name/cust": f"cust={root}/cust.img",
                "/tmp/skyward-patch.": f"{scratch}/skyward-patch.",
                "done </proc/mounts": f"done <{root}/mounts",
            }
            for before, after in replacements.items():
                self.assertIn(before, script)
                script = script.replace(before, after)
            self.assertNotIn("/dev/block/", script.replace('case "$source" in /dev/block/*)', ""))
            self.assertNotIn("/data/", script)
            (package / "install.sh").write_text(script)
            checksums = []
            for path in sorted(package.rglob("*")):
                if path.is_file():
                    digest = hashlib.sha256(path.read_bytes()).hexdigest()
                    checksums.append(f"{digest}  {path.relative_to(package)}")
            (package / "SHA256SUMS").write_text("\n".join(checksums) + "\n")
            archive = root / "seed.zip"
            archive.write_bytes(b"fixture")
            result = subprocess.run(
                ["/bin/sh", str(package / "install.sh")]
                + (["--retain", "_b"] if retain else ["--install"]),
                env={**os.environ, "INSTALLER_FIXTURE": str(root), "INSTALLER_FAIL": failure,
                     "SKYWARD_REINSTALL_ZIP": str(archive)},
                capture_output=True, text=True, timeout=45,
            )
            self.assertEqual(result.returncode, 1 if failure else 0, result.stdout + result.stderr)
            self.assertEqual((store / "unrelated.txt").read_bytes(), b"retain")
            self.assertEqual((root / "boot_b.img").read_bytes(),
                             original if failure else original + b"-patched")
            payload = store / "skyward/prototype-v1/recovery"
            if failure:
                self.assertEqual(payload.exists(), existing)
                if existing:
                    self.assertEqual(payload.read_bytes(), b"old")
            else:
                self.assertEqual(payload.read_bytes(), b"new")
                self.assertIn("Recovery route retained" if retain else "No persistent backup", result.stdout)
            self.assertEqual(list(scratch.iterdir()), [], result.stdout + result.stderr)
            self.assertFalse(list(root.rglob("Skyward-backups")))

    def test_fresh_install_without_data(self):
        self.run_install()

    def test_update_without_data(self):
        self.run_install(existing=True)

    def test_automatic_retention_without_data(self):
        self.run_install(existing=True, retain=True)

    def test_payload_failure_restores_previous_directory(self):
        self.run_install(existing=True, failure="payload")

    def test_boot_failure_restores_boot_and_previous_directory(self):
        self.run_install(existing=True, failure="boot")

    def test_fresh_failure_removes_new_directory(self):
        self.run_install(failure="boot")


if __name__ == "__main__":
    unittest.main()
