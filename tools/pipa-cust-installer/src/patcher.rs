use anyhow::{ensure, Context, Result};
use sha2::{Digest, Sha256};
use skyward_bootimg::{BootImage, Ramdisk};
use std::{env, fs, path::Path};
#[path = "../../../crates/skyward-pack/src/avb.rs"]
mod avb;

fn digest(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }

fn recovery_properties(original: &[u8]) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(original).context("boot properties are not UTF-8")?;
    let replaced = [
        "ro.build.version.release", "ro.build.version.security_patch", "ro.vendor.build.security_patch",
        "ro.secure", "ro.adb.secure", "ro.debuggable", "ro.ota.allow_downgrade", "ro.skyward.binder.header",
    ];
    let mut lines: Vec<&str> = text.lines().filter(|line| {
        !line.split_once('=').is_some_and(|(key, _)| replaced.contains(&key.trim()))
    }).collect();
    while lines.last().is_some_and(|line| line.is_empty()) { lines.pop(); }
    let mut result = lines.join("\n");
    result.push_str("\n\nro.secure=0\nro.adb.secure=0\nro.debuggable=1\nro.ota.allow_downgrade=true\nro.skyward.binder.header=SYST\n");
    Ok(result.into_bytes())
}

fn main() -> Result<()> {
    let args: Vec<_> = env::args().collect();
    ensure!(args.len() == 4, "boot-patcher ORIGINAL_BOOT DATA_DIRECTORY OUTPUT_BOOT");
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
    let expected = fs::read_to_string(data.join("original-targets.txt"))?;
    ensure!(overlay.entries.len() == expected.lines().count(), "overlay manifest mismatch");
    for entry in &mut overlay.entries {
        let line = expected.lines().find(|line| line.split_once(' ').is_some_and(|(_, name)| name == entry.name))
            .context("overlay target missing")?;
        let (old_hash, _) = line.split_once(' ').unwrap();
        match rd.find(&entry.name) {
            Some(old) => {
                let known = digest(&old.data) == old_hash || (old.data == entry.data && old.mode == entry.mode);
                if !known && entry.name == "prop.default" {
                    entry.data = recovery_properties(&old.data)?;
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
    for entry in overlay.entries {
        if let Some(at) = rd.entries.iter().position(|e| e.name == entry.name) {
            rd.entries[at] = entry;
        } else {
            rd.entries.push(entry);
        }
    }
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
        let once = recovery_properties(source).unwrap();
        let text = std::str::from_utf8(&once).unwrap();
        assert!(text.contains("ro.product.device=pipa\nro.custom.kernel=test\n"));
        assert!(!text.contains("ro.build.version.release="));
        assert!(!text.contains("ro.secure=1"));
        assert!(text.contains("ro.secure=0\n"));
        assert_eq!(once, recovery_properties(&once).unwrap());
    }
}
