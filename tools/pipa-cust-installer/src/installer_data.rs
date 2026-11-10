use anyhow::{ensure, Context, Result};
use skyward_bootimg::{BootImage, Ramdisk};
use std::{env, fs, path::Path};

mod overlay;

const INIT_RC: &str = "system/etc/init/hw/init.rc";
const RECOVERY: &str = "system/bin/recovery";
const LOADER: &str = "system/bin/recovery-loader";

fn build_overlay(base: &Ramdisk, mut target: Ramdisk, loader: Vec<u8>) -> Result<(Ramdisk, String)> {
    ensure!(loader.starts_with(b"\x7fELF"), "loader is not ELF");
    let first_init = base.find("system/bin/init").context("base init missing")?;
    let target_init = target.find("system/bin/init").context("target init missing")?;
    ensure!(first_init.data == target_init.data && first_init.mode == target_init.mode,
        "first-stage init changed");
    let init = target.entries.iter_mut().find(|e| e.name == INIT_RC).context("init rules missing")?;
    let text = std::str::from_utf8(&init.data)?;
    let needle = "service recovery /system/bin/recovery\n";
    ensure!(text.matches(needle).count() == 1, "unexpected recovery service");
    init.data = text.replacen(needle, "service recovery /system/bin/recovery-loader\n", 1).into_bytes();
    ensure!(target.find(LOADER).is_none() && target.find(overlay::MANIFEST).is_none(),
        "target already has cust routing");
    let mut entry = target.find(RECOVERY).context("fallback recovery missing")?.clone();
    entry.name = LOADER.into();
    entry.data = loader;
    target.entries.push(entry);
    for old in &base.entries {
        ensure!(target.find(&old.name).is_some(), "base entry removed: {}", old.name);
    }
    target.entries.retain(|entry| {
        base.find(&entry.name).is_none_or(|old|
            old.data != entry.data || old.mode != entry.mode ||
            old.uid != entry.uid || old.gid != entry.gid)
    });
    ensure!(target.find("system/bin/init").is_none(), "overlay replaces first-stage init");
    let stamp = overlay::manifest(&target.entries);
    overlay::parse_manifest(&stamp)?;
    target.add(overlay::MANIFEST, 0o644, stamp.into_bytes())?;
    let expected = target.entries.iter().map(|entry| {
        let hash = base.find(&entry.name).map(|old| overlay::digest(&old.data)).unwrap_or("-".into());
        format!("{hash} {}\n", entry.name)
    }).collect();
    Ok((target, expected))
}

fn main() -> Result<()> {
    let args: Vec<_> = env::args().collect();
    ensure!(args.len() == 5, "make-installer-data BASE CURRENT_BOOT CURRENT_LOADER OUT");
    let base = BootImage::read(Path::new(&args[1]))?;
    let current = BootImage::read(Path::new(&args[2]))?;
    ensure!(base.kernel == current.kernel, "base kernel changed");
    let (overlay, expected) = build_overlay(
        &Ramdisk::decode(&base.ramdisk)?, Ramdisk::decode(&current.ramdisk)?, fs::read(&args[3])?,
    )?;
    let out = Path::new(&args[4]);
    fs::create_dir_all(out)?;
    fs::write(out.join("overlay.gz"), overlay.encode_like(&base.ramdisk)?)?;
    fs::write(out.join("original-targets.txt"), expected)?;
    println!("Generated {} overlay entries from this build's boot and loader.", overlay.entries.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use skyward_bootimg::Entry;

    fn ramdisk() -> Ramdisk {
        let entry = |name: &str, data: &[u8]| Entry {
            name: name.into(), data: data.into(), mode: 0o100755,
            uid: 0, gid: 0, mtime: 0, nlink: 1, dev: (0, 0), rdev: (0, 0),
        };
        Ramdisk { entries: vec![
            entry("system/bin/init", b"original init"),
            entry(INIT_RC, b"on init\n\nservice recovery /system/bin/recovery\n    class main\n"),
            entry(RECOVERY, b"old recovery"),
        ] }
    }

    #[test]
    fn routes_only_recovery_and_uses_current_payload_and_loader() {
        let base = ramdisk();
        let mut current = ramdisk();
        current.replace(RECOVERY, b"new recovery".to_vec()).unwrap();
        let (overlay, expected) = build_overlay(&base, current, b"\x7fELFnew loader".to_vec()).unwrap();
        assert_eq!(overlay.find(RECOVERY).unwrap().data, b"new recovery");
        assert_eq!(overlay.find(LOADER).unwrap().data, b"\x7fELFnew loader");
        assert!(overlay.find("system/bin/init").is_none());
        assert!(expected.contains("- skyward/boot-overlay.sha256\n"));
        let init = &overlay.find(INIT_RC).unwrap().data;
        assert!(std::str::from_utf8(init).unwrap().contains("service recovery /system/bin/recovery-loader\n"));
    }

    #[test]
    fn refuses_changed_first_stage_init_and_unknown_service_layout() {
        let mut current = ramdisk();
        current.replace("system/bin/init", b"other init".to_vec()).unwrap();
        assert!(build_overlay(&ramdisk(), current, b"\x7fELFloader".to_vec()).is_err());
        let mut current = ramdisk();
        current.replace(INIT_RC, b"service recovery /other/program\n".to_vec()).unwrap();
        assert!(build_overlay(&ramdisk(), current, b"\x7fELFloader".to_vec()).is_err());
    }
}
