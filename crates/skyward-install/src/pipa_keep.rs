//! Post-install boot repair using a verified installer staged in RAM.

use anyhow::{ensure, Context, Result};
use sha2::{Digest, Sha256};
use skyward_vab::Slot;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const ARCHIVE: &str = "/skyward/reinstall/installer.zip";
pub const DIGEST: &str = "/skyward/reinstall/installer.sha256";
pub const LOG: &str = "/skyward/run/reinstall.log";

pub fn available() -> bool {
    Path::new(ARCHIVE).is_file() && Path::new(DIGEST).is_file()
}

#[cfg(target_os = "android")]
pub fn next_slot() -> Result<Slot> {
    match skyward_vab::boot::next_slot() {
        Ok(slot) => return Ok(slot),
        Err(error) => {
            let mut log = OpenOptions::new().create(true).append(true).open(LOG)?;
            writeln!(log, "boot control lookup failed; requesting lazy service: {error:#}")?;
        }
    }
    skyward_sys::set_prop("ctl.interface_start",
        "aidl/android.hardware.boot.IBootControl/default")
        .context("start pipa boot control interface")?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match skyward_vab::boot::next_slot() {
            Ok(slot) => return Ok(slot),
            Err(error) if std::time::Instant::now() >= deadline =>
                return Err(error).context("wait for pipa boot control registration"),
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
        }
    }
}

pub struct Kept {
    stage: tempfile::TempDir,
    directory: PathBuf,
    before: [[u8; 32]; 2],
}

fn digest(path: &Path) -> Result<[u8; 32]> {
    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    crate::image::drop_cache(&file)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let size = file.read(&mut buffer)?;
        if size == 0 { break; }
        hash.update(&buffer[..size]);
    }
    Ok(hash.finalize().into())
}

pub fn selected(before: [[u8; 32]; 2], after: [[u8; 32]; 2], next: Slot) -> Vec<Slot> {
    [Slot::A, Slot::B].into_iter()
        .filter(|slot| *slot == next || before[slot.number() as usize] != after[slot.number() as usize])
        .collect()
}

impl Kept {
    pub fn hold(directory: &Path) -> Result<Self> {
        Self::capture(directory, Path::new(ARCHIVE), Path::new(DIGEST), Path::new("/tmp"))
    }

    fn capture(directory: &Path, archive: &Path, expected: &Path, ram: &Path) -> Result<Self> {
        ensure!(fs::metadata(archive)?.len() <= 128 * 1024 * 1024, "reinstall archive too large");
        let bytes = fs::read(archive)?;
        let expected = fs::read_to_string(expected)?;
        ensure!(format!("{:x}", Sha256::digest(&bytes)) == expected.trim(),
            "reinstall archive hash mismatch");
        let stage = tempfile::Builder::new().prefix("skyward-retain-").tempdir_in(ram)?;
        fs::write(stage.path().join("installer.zip"), &bytes)?;
        let root = stage.path().join("package");
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
        // Only the build's hash-verified archive is extracted, before the ROM runs.
        zip.extract(&root)?;
        for tool in ["boot-patcher", "recovery-loader", "cust-readonly"] {
            fs::set_permissions(root.join("tools").join(tool), fs::Permissions::from_mode(0o755))?;
        }
        ensure!(root.join("install.sh").is_file(), "reinstall script missing");
        let before = [
            digest(&crate::image::device(directory, "boot", Slot::A))?,
            digest(&crate::image::device(directory, "boot", Slot::B))?,
        ];
        Ok(Self { stage, directory: directory.into(), before })
    }

    pub fn wanted(&self, next: Slot) -> Result<Vec<Slot>> {
        let after = [
            digest(&crate::image::device(&self.directory, "boot", Slot::A))?,
            digest(&crate::image::device(&self.directory, "boot", Slot::B))?,
        ];
        Ok(selected(self.before, after, next))
    }

    fn install(&self, slot: Slot, check: bool) -> Result<()> {
        let root = self.stage.path().join("package");
        let output = Command::new("/system/bin/sh").arg(root.join("install.sh"))
            .arg(if check { "--retain-check" } else { "--retain" })
            .arg(match slot { Slot::A => "_a", Slot::B => "_b" })
            .env("SKYWARD_REINSTALL_ZIP", self.stage.path().join("installer.zip"))
            .env_remove("OUTFD")
            .current_dir(&root).output().context("run the RAM recovery installer")?;
        let mut log = OpenOptions::new().create(true).append(true).open(LOG)?;
        writeln!(log, "slot {slot:?}, check={check}, status={}", output.status)?;
        log.write_all(&output.stdout)?;
        log.write_all(&output.stderr)?;
        ensure!(output.status.success(), "Skyward retention failed for {slot:?}; see {LOG}: {}",
            String::from_utf8_lossy(&output.stdout).trim());
        Ok(())
    }

    pub fn write(&self, slots: &[Slot], progress: &mut dyn FnMut(u64, u64)) -> Result<()> {
        // Reject unsupported targets before writing either slot.
        for &slot in slots { self.install(slot, true)?; }
        progress(0, slots.len() as u64);
        for (index, &slot) in slots.iter().enumerate() {
            self.install(slot, false)?;
            progress((index + 1) as u64, slots.len() as u64);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retain_next_and_changed_boots_without_touching_an_unchanged_other_slot() {
        let before = [[1; 32], [2; 32]];
        assert_eq!(selected(before, before, Slot::B), [Slot::B]);
        assert_eq!(selected(before, [[3; 32], [2; 32]], Slot::B), [Slot::A, Slot::B]);
        assert_eq!(selected(before, before, Slot::A), [Slot::A]);
    }

    #[test]
    fn corrupted_archive_is_rejected_before_boot_access() {
        let ram = tempfile::tempdir().unwrap();
        let zip = ram.path().join("bad.zip");
        let expected = ram.path().join("digest");
        fs::write(&zip, b"not an archive").unwrap();
        fs::write(&expected, "0".repeat(64)).unwrap();
        let result = Kept::capture(Path::new("/no-block-devices"), &zip, &expected, ram.path());
        assert!(result.err().unwrap().to_string().contains("hash mismatch"));
    }

    #[test]
    fn captured_installer_survives_source_removal_and_detects_the_changed_slot() {
        let ram = tempfile::tempdir().unwrap();
        let source = ram.path().join("source.zip");
        let expected = ram.path().join("digest");
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for name in ["install.sh", "tools/boot-patcher", "tools/recovery-loader", "tools/cust-readonly"] {
            zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(b"fixture").unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();
        fs::write(&source, &bytes).unwrap();
        fs::write(&expected, format!("{:x}", Sha256::digest(&bytes))).unwrap();
        fs::write(ram.path().join("boot_a"), b"boot A").unwrap();
        fs::write(ram.path().join("boot_b"), b"boot B").unwrap();
        let kept = Kept::capture(ram.path(), &source, &expected, ram.path()).unwrap();
        fs::remove_file(source).unwrap();
        fs::remove_file(expected).unwrap();
        assert_eq!(fs::read(kept.stage.path().join("installer.zip")).unwrap(), bytes);
        assert!(kept.stage.path().join("package/install.sh").is_file());
        assert_eq!(kept.wanted(Slot::B).unwrap(), [Slot::B]);
        fs::write(ram.path().join("boot_a"), b"new kernel").unwrap();
        assert_eq!(kept.wanted(Slot::B).unwrap(), [Slot::A, Slot::B]);
    }
}
