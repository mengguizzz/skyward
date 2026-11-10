#[cfg(target_os = "android")]
fn main() -> anyhow::Result<()> {
    use anyhow::{ensure, Context};
    use skyward_block::{dm, mount};
    use std::fs::{self, File};
    use std::path::PathBuf;
    use std::time::Duration;

    ensure!(skyward_sys::prop("ro.product.device").as_deref() == Some("pipa"), "pipa only");
    let slot = skyward_vab::current_slot()?.context("no booted slot")?;
    ensure!(skyward_vab::snapshot_state()? == skyward_vab::SnapshotState::None,
        "probe requires no pending snapshot");
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
        // File::metadata uses fstat, avoiding the independently reproduced
        // path-stat reference leak. Do not call path.is_dir() here.
        ensure!(File::open(&point)?.metadata()?.is_dir(), "not a directory");
        let count = fs::read_dir(&point)?.collect::<Result<Vec<_>, _>>()?.len();
        println!("Read {count} directory entries without path-stat");
        mount::umount(&point)?;
        mounted = false;
        println!("PASS: ordinary unmount succeeded after init observed the mount");
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
