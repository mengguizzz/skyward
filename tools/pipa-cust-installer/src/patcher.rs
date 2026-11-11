use anyhow::{ensure, Context, Result};
use sha2::{Digest, Sha256};
use skyward_bootimg::{BootImage, Ramdisk};
use std::{env, fs, path::Path};
#[path = "../../../crates/skyward-pack/src/avb.rs"]
mod avb;
mod overlay;
use overlay::{digest, Record};

fn previous_overlay(rd: &Ramdisk) -> Result<std::collections::BTreeMap<String, Record>> {
    let Some(stamp) = rd.find(overlay::MANIFEST) else { return Ok(Default::default()) };
    ensure!(stamp.mode == 0o100644 && stamp.uid == 0 && stamp.gid == 0, "invalid overlay stamp metadata");
    let records = overlay::parse_manifest(std::str::from_utf8(&stamp.data)?)?;
    for (name, record) in &records {
        let entry = rd.find(name).with_context(|| format!("previous overlay entry missing: {name}"))?;
        // Properties can be changed by other installers; merge their unrelated keys below.
        ensure!(name == "prop.default" || record.matches(&Record::of(entry)),
            "previous overlay entry changed: {name}");
    }
    Ok(records)
}

fn recovery_properties(original: &[u8], desired: &[u8]) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(original).context("boot properties are not UTF-8")?;
    let desired = std::str::from_utf8(desired).context("overlay properties are not UTF-8")?;
    let replaced = [
        "ro.build.version.release", "ro.build.version.security_patch", "ro.vendor.build.security_patch",
        "ro.secure", "ro.adb.secure", "ro.debuggable", "ro.ota.allow_downgrade", "ro.skyward.binder.header",
    ];
    let mut lines: Vec<&str> = text.lines().filter(|line| {
        !line.split_once('=').is_some_and(|(key, _)| replaced.contains(&key.trim()))
    }).collect();
    while lines.last().is_some_and(|line| line.is_empty()) { lines.pop(); }
    let mut result = lines.join("\n");
    result.push_str("\n\n");
    for line in desired.lines() {
        if line.split_once('=').is_some_and(|(key, _)| replaced[3..].contains(&key.trim())) {
            result.push_str(line);
            result.push('\n');
        }
    }
    Ok(result.into_bytes())
}

fn apply_overlay(rd: &mut Ramdisk, mut overlay: Ramdisk, expected: &str) -> Result<()> {
    ensure!(overlay.entries.len() == expected.lines().count(), "overlay manifest mismatch");
    ensure!(overlay.find("system/bin/init").is_none(), "overlay must retain first-stage init");
    let previous = previous_overlay(rd)?;
    let legacy = overlay::parse_manifest(include_str!("legacy-overlay.sha256"))?;
    for name in previous.keys() {
        ensure!(overlay.find(name).is_some(), "new overlay omits previously managed file: {name}");
    }
    for entry in &mut overlay.entries {
        let line = expected.lines().find(|line| line.split_once(' ').is_some_and(|(_, name)| name == entry.name))
            .context("overlay target missing")?;
        let (old_hash, _) = line.split_once(' ').unwrap();
        match rd.find(&entry.name) {
            Some(old) => {
                let record = Record::of(old);
                let known = digest(&old.data) == old_hash ||
                    Record::of(entry) == record ||
                    previous.get(&entry.name).is_some_and(|known| known.matches(&record)) ||
                    legacy.get(&entry.name).is_some_and(|known| known.matches(&record)) ||
                    (entry.name == overlay::MANIFEST && !previous.is_empty());
                if entry.name == "prop.default" {
                    entry.data = recovery_properties(&old.data, &entry.data)?;
                    entry.mode = old.mode;
                    entry.uid = old.uid;
                    entry.gid = old.gid;
                } else {
                    ensure!(known, "conflicting ramdisk file: {}", entry.name);
                }
            }
            None => ensure!(old_hash == "-", "required ramdisk file missing: {}", entry.name),
        }
    }
    let stamp = overlay::manifest(&overlay.entries);
    let stamp_entry = overlay.entries.iter_mut().find(|e| e.name == overlay::MANIFEST)
        .context("generated overlay stamp missing")?;
    stamp_entry.data = stamp.into_bytes();
    for entry in overlay.entries {
        if let Some(at) = rd.entries.iter().position(|e| e.name == entry.name) {
            rd.entries[at] = entry;
        } else {
            rd.entries.push(entry);
        }
    }
    Ok(())
}

fn add_reinstall(overlay: &mut Ramdisk, expected: &mut String, bytes: Vec<u8>) -> Result<()> {
    ensure!(bytes.starts_with(b"PK\x03\x04") && bytes.len() <= 128 * 1024 * 1024,
        "invalid reinstall archive");
    let original: std::collections::BTreeSet<_> =
        overlay.entries.iter().map(|entry| entry.name.clone()).collect();
    for (name, bytes) in [
        ("skyward/reinstall/installer.sha256", format!("{:x}\n", Sha256::digest(&bytes)).into_bytes()),
        ("skyward/reinstall/installer.zip", bytes),
    ] {
        ensure!(overlay.find(name).is_none(), "reinstall archive must use a non-recursive seed overlay");
        overlay.add(name, 0o644, bytes)?;
    }
    for entry in &overlay.entries {
        if !original.contains(&entry.name) {
            expected.push_str(&format!("- {}\n", entry.name));
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = env::args().collect();
    ensure!(args.len() == 4 || args.len() == 5,
        "boot-patcher ORIGINAL_BOOT DATA_DIRECTORY OUTPUT_BOOT [REINSTALL_ZIP]");
    let original = fs::read(&args[1])?;
    ensure!(original.len() == 192 * 1024 * 1024, "expected 192 MiB boot");
    ensure!(u32::from_le_bytes(original[40..44].try_into()?) == 3, "expected header v3");
    let mut image = BootImage::parse(&original)?;
    let data = Path::new(&args[2]);
    let mut rd = Ramdisk::decode(&image.ramdisk)?;
    let init = rd.find("system/bin/init").context("TWRP init missing")?;
    ensure!(init.data.starts_with(b"\x7fELF"), "TWRP init is not an ELF executable");
    let old_line = std::str::from_utf8(&original[44..1580])?.split('\0').next().unwrap();
    ensure!(!old_line.split_ascii_whitespace().any(|a| a.starts_with("twrpfastboot=")),
        "temporary forced-recovery boot cannot be installed; restore normal TWRP boot first");
    let mut overlay = Ramdisk::decode(&fs::read(data.join("overlay.gz"))?)?;
    let mut expected = fs::read_to_string(data.join("original-targets.txt"))?;
    if let Some(archive) = args.get(4) {
        add_reinstall(&mut overlay, &mut expected, fs::read(archive)?)?;
    }
    apply_overlay(&mut rd, overlay, &expected)?;
    let mut tokens: Vec<_> = old_line.split_ascii_whitespace().collect();
    ensure!(!tokens.iter().any(|t| t.starts_with("cnss2.disable_nv_mac=") && *t != "cnss2.disable_nv_mac=1"),
        "conflicting WLAN kernel parameter");
    if !tokens.contains(&"cnss2.disable_nv_mac=1") { tokens.push("cnss2.disable_nv_mac=1"); }
    let line = tokens.join(" ");
    ensure!(line.len() < image.header.cmdline.len(), "kernel command line too long");
    image.header.cmdline.fill(0);
    image.header.cmdline[..line.len()].copy_from_slice(line.as_bytes());
    image.ramdisk = rd.encode_like(&image.ramdisk)?;
    let bytes = image.to_bytes()?;
    let result = avb::footed(&bytes, "boot", original.len(), &Sha256::digest(&bytes))?;
    let rebuilt = BootImage::parse(&result)?;
    ensure!(rebuilt.kernel == BootImage::parse(&original)?.kernel, "kernel changed");
    let old = Ramdisk::decode(&BootImage::parse(&original)?.ramdisk)?;
    let new = Ramdisk::decode(&rebuilt.ramdisk)?;
    for entry in &old.entries {
        let changed = expected.lines().any(|s| s.split_once(' ').unwrap().1 == entry.name);
        if !changed {
            let after = new.find(&entry.name).context("entry removed")?;
            ensure!(after.data == entry.data && after.mode == entry.mode &&
                after.uid == entry.uid && after.gid == entry.gid, "unrelated entry changed");
        }
    }
    fs::write(&args[3], result)?;
    println!("Patched recovery userspace; original kernel, first-stage init and unrelated ramdisk files retained.");
    println!("Normal kernel arguments retained; no forced-recovery flag added.");
    println!("Device eligibility is checked by the pipa installer; no kernel or init hash allowlist.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_base_properties_survive_and_recovery_overrides_are_idempotent() {
        let source = b"ro.product.device=pipa\nro.custom.kernel=test\nro.build.version.release=14\nro.secure=1\n";
        let desired = b"ro.secure=0\nro.adb.secure=0\nro.debuggable=1\nro.ota.allow_downgrade=true\nro.skyward.binder.header=SYST\n";
        let once = recovery_properties(source, desired).unwrap();
        let text = std::str::from_utf8(&once).unwrap();
        assert!(text.contains("ro.product.device=pipa\nro.custom.kernel=test\n"));
        assert!(!text.contains("ro.build.version.release="));
        assert!(!text.contains("ro.secure=1"));
        assert!(text.contains("ro.secure=0\n"));
        assert_eq!(once, recovery_properties(&once, desired).unwrap());
    }

    fn fixture() -> Ramdisk {
        let mut rd = Ramdisk { entries: Vec::new() };
        rd.add("system/bin/init", 0o755, b"original init".to_vec()).unwrap();
        rd.add("system/bin/recovery", 0o755, b"original recovery".to_vec()).unwrap();
        rd.add("prop.default", 0o644, b"ro.custom.kernel=yes\nro.secure=1\n".to_vec()).unwrap();
        rd
    }

    fn update(base: &Ramdisk, version: &[u8]) -> (Ramdisk, String) {
        let mut overlay = Ramdisk { entries: Vec::new() };
        overlay.add("system/bin/recovery", 0o755, version.to_vec()).unwrap();
        overlay.add("skyward/device.toml", 0o644, version.to_vec()).unwrap();
        overlay.add("prop.default", 0o644, b"ro.secure=0\nro.skyward.binder.header=SYST\n".to_vec()).unwrap();
        overlay.add(overlay::MANIFEST, 0o644, overlay::manifest(&overlay.entries).into_bytes()).unwrap();
        let expected = overlay.entries.iter().map(|e| {
            format!("{} {}\n", base.find(&e.name).map(|old| digest(&old.data)).unwrap_or("-".into()), e.name)
        }).collect();
        (overlay, expected)
    }

    #[test]
    fn fresh_repeat_and_changed_source_upgrade_keep_init_and_custom_properties() {
        let base = fixture();
        let mut installed = fixture();
        for version in [b"build one".as_slice(), b"build one", b"build two"] {
            let (overlay, expected) = update(&base, version);
            apply_overlay(&mut installed, overlay, &expected).unwrap();
            assert_eq!(installed.find("system/bin/recovery").unwrap().data, version);
            assert_eq!(installed.find("skyward/device.toml").unwrap().data, version);
            assert_eq!(installed.find("system/bin/init").unwrap().data, b"original init");
            assert!(std::str::from_utf8(&installed.find("prop.default").unwrap().data).unwrap().contains("ro.custom.kernel=yes\n"));
            previous_overlay(&installed).unwrap();
        }
    }

    #[test]
    fn unknown_changes_and_unstamped_target_collisions_are_refused() {
        let base = fixture();
        let mut installed = fixture();
        let (overlay, expected) = update(&base, b"build one");
        apply_overlay(&mut installed, overlay, &expected).unwrap();
        installed.replace("skyward/device.toml", b"unknown change".to_vec()).unwrap();
        let (overlay, expected) = update(&base, b"build two");
        assert!(apply_overlay(&mut installed, overlay, &expected).is_err());
        let mut other = fixture();
        other.add("skyward/device.toml", 0o644, b"third-party file".to_vec()).unwrap();
        let (overlay, expected) = update(&base, b"build two");
        assert!(apply_overlay(&mut other, overlay, &expected).is_err());
    }

    #[test]
    fn seed_reinstall_retains_its_archive_and_managed_stamp_on_repeat() {
        let base = fixture();
        let mut installed = fixture();
        for _ in 0..2 {
            let (mut overlay, mut expected) = update(&base, b"same build");
            add_reinstall(&mut overlay, &mut expected, b"PK\x03\x04seed".to_vec()).unwrap();
            apply_overlay(&mut installed, overlay, &expected).unwrap();
            assert_eq!(installed.find("skyward/reinstall/installer.zip").unwrap().data,
                b"PK\x03\x04seed");
            assert!(previous_overlay(&installed).unwrap()
                .contains_key("skyward/reinstall/installer.zip"));
            assert_eq!(installed.find("system/bin/init").unwrap().data, b"original init");
        }
        let (mut overlay, mut expected) = update(&base, b"same build");
        assert!(add_reinstall(&mut overlay, &mut expected, b"bad zip".to_vec()).is_err());
    }
}
