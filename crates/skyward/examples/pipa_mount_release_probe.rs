#[cfg(target_os = "android")]
#[allow(dead_code)]
#[path = "../src/private_data.rs"]
mod private_data;

#[cfg(target_os = "android")]
fn main() -> anyhow::Result<()> {
    use anyhow::{ensure, Context};
    use skyward_block::{dm, mount};
    use std::fs;
    use std::path::PathBuf;
    use std::os::unix::process::CommandExt;
    use std::process::Command;
    use std::time::Duration;

    ensure!(skyward_sys::prop("ro.product.device").as_deref() == Some("pipa"), "pipa only");
    let slot = skyward_vab::current_slot()?.context("no booted slot")?;
    ensure!(skyward_vab::snapshot_state()? == skyward_vab::SnapshotState::None,
        "probe requires no pending snapshot");
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        ensure!(args.len() == 2 && args[0] == "--release-live"
            && matches!(args[1].as_str(), "vendor" | "odm"), "expected --release-live vendor|odm");
        let name = format!("{}{}", args[1], slot.suffix());
        skyward_vab::unmount_one(slot, &args[1])?;
        ensure!(dm::status(&name)?.is_none(), "{name} is still mapped");
        println!("PASS: ordinary unmount and DM removal for {name}");
        return Ok(());
    }
    let source = format!("odm{}", slot.suffix());
    let lines = dm::table(&source)?;
    let mut targets = Vec::new();
    let mut end = 0;
    for line in &lines {
        ensure!(line.kind == "linear" && line.start == end,
            "probe only supports a contiguous linear odm table");
        let fields: Vec<_> = line.text.split_whitespace().collect();
        ensure!(fields.len() == 2, "unexpected linear target");
        let (major, minor) = fields[0].split_once(':').context("bad device number")?;
        targets.push(dm::Target::Linear {
            sectors: line.sectors,
            device: (major.parse()?, minor.parse()?),
            offset: fields[1].parse()?,
        });
        end = end.checked_add(line.sectors).context("table size overflow")?;
    }
    ensure!(!targets.is_empty(), "odm has no targets");
    let name = format!("skyward-probe-{}", std::process::id());
    ensure!(dm::status(&name)?.is_none(), "probe mapping already exists");
    let root = PathBuf::from(format!("/tmp/{name}"));
    fs::create_dir(&root)?;
    let point = root.join("mounted");
    fs::create_dir(&point)?;
    let device = dm::create(&name, &targets, true)?;
    mount::enable_source_aliases();
    let mut mounted = false;
    let run = (|| -> anyhow::Result<()> {
        ensure!(dm::readonly(&name)?, "probe mapping must be read-only");
        let ty = mount::mount_ro(&device, &point)?;
        mounted = true;
        println!("Mounted a temporary read-only {ty} view of {source} using the source-alias backend");
        std::thread::sleep(Duration::from_secs(1));
        let mountinfo = fs::read_to_string("/proc/self/mountinfo")?;
        let row = mountinfo.lines().find(|row| row.split_whitespace().nth(4) == point.to_str())
            .context("probe mount missing from mountinfo")?;
        let source_path = row.split_once(" - ").context("invalid mountinfo")?.1
            .split_whitespace().nth(1).context("mount source missing")?;
        ensure!(source_path.starts_with("/skyward/run/block-sources/"), "mount did not use an alias");
        println!("Mount source recorded by kernel: {source_path}");
        // Exercise the same directory check used for /odm and /vendor,
        // against our temporary mount rather than either live mount.
        let base = point.strip_prefix("/")?.to_str().context("non-UTF8 probe path")?;
        ensure!(skyward_vab::mount_point(base) == point, "mount-point lookup failed");
        // Force SYS_fstat: newer Rust may implement File::metadata through
        // fstatat(AT_EMPTY_PATH), which also leaks on the retained kernel.
        ensure!(mount::path_metadata(&point)?.is_dir(), "path metadata lost directory type");
        ensure!(fs::metadata(&point)?.is_dir(), "path compatibility lost directory type");
        ensure!(fs::File::open(&point)?.metadata()?.is_dir(), "descriptor compatibility lost directory type");
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        let fd = fs::File::open(&point)?;
        use std::os::fd::AsRawFd;
        ensure!(unsafe { libc::fstatat(fd.as_raw_fd(), c"".as_ptr(), &mut st,
            libc::AT_EMPTY_PATH | libc::AT_NO_AUTOMOUNT) } == 0, "empty-path compatibility failed");
        drop(fd);
        // installed_props checks this path on every mount-page refresh.
        // Exercise that check on our independent view of the same odm.
        let properties = point.join("etc/build.prop");
        match mount::path_metadata(&properties) {
            Ok(metadata) => {
                ensure!(metadata.is_file(), "build.prop is not a file");
                let mut props = std::collections::HashMap::new();
                rsproperties::load_properties_from_file(&properties, None, "u:r:init:s0", &mut props)?;
                let _ = fs::read_to_string(&properties)?;
                println!("Parsed {} properties using the patched property parser", props.len());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        // The mount page also scans every process's descriptors and cwd.
        let _ = mount::holders()?;
        let fixtures = tempfile::Builder::new().prefix("pipa-props-").tempdir_in("/tmp")?;
        let base = fixtures.path().join("base.prop");
        let main = fixtures.path().join("main.prop");
        let alias = fixtures.path().join("alias.prop");
        fs::write(&base, "probe.value=base\n")?;
        fs::write(&main, format!(
            "probe.base={}\nimport ${{probe.base}}\nprobe.value=override\nimport {}\n",
            base.display(), alias.display()
        ))?;
        std::os::unix::fs::symlink(&main, &alias)?;
        let mut props = std::collections::HashMap::new();
        rsproperties::load_properties_from_file(&main, None, "u:r:init:s0", &mut props)?;
        ensure!(props.get("probe.value").map(String::as_str) == Some("override"),
            "property import, last-wins override or symlink cycle handling changed");
        println!("PASS: property imports, overrides and symlink cycles");
        let count = fs::read_dir(&point)?.collect::<Result<Vec<_>, _>>()?.len();
        println!("Read {count} directory entries without path-stat");
        // Reproduce cnss inheriting a view that is invisible in the parent.
        let mut command = Command::new("/system/bin/sleep");
        command.arg("30").current_dir("/");
        unsafe {
            command.pre_exec(|| {
                if libc::unshare(libc::CLONE_NEWNS) != 0
                    || libc::mount(std::ptr::null(), c"/".as_ptr(), std::ptr::null(),
                        libc::MS_REC | libc::MS_PRIVATE, std::ptr::null()) != 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut holder = command.spawn()?;
        let released = (|| -> anyhow::Result<()> {
            mount::umount(&point)?;
            mounted = false;
            println!("PASS: ordinary unmount succeeded after init observed the mount");
            let error = dm::remove(&name).expect_err("private namespace must retain the probe mapping");
            ensure!(error.downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.raw_os_error() == Some(libc::EBUSY)),
                "unexpected DM removal error: {error:#}");
            println!("REPRODUCED: parent unmount succeeded but inherited namespace keeps DM busy");
            private_data::release_slot_mounts(holder.id(), &[point.clone()])?;
            Ok(())
        })();
        let stopped = holder.kill();
        let waited = holder.wait();
        released?;
        stopped?;
        waited?;
        println!("PASS: released only the temporary private view");
        Ok(())
    })();
    let cleanup_mount = if mounted { mount::umount(&point) } else { Ok(()) };
    let cleanup_device = dm::remove(&name);
    if cleanup_mount.is_ok() && cleanup_device.is_ok() {
        fs::remove_dir(&point)?;
        fs::remove_dir(&root)?;
    }
    run?;
    cleanup_mount.context("cleanup probe mount")?;
    cleanup_device.context("cleanup probe mapping")?;
    ensure!(dm::status(&name)?.is_none(), "probe mapping still exists");
    println!("PASS: temporary device-mapper mapping removed; original {source} unchanged");
    Ok(())
}

#[cfg(not(target_os = "android"))]
fn main() {}
