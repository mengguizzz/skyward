#[cfg(target_os = "android")]
#[allow(dead_code)]
#[path = "../src/private_data.rs"]
mod private_data;

#[cfg(target_os = "android")]
fn main() -> anyhow::Result<()> {
    use anyhow::{ensure, Context};
    use skyward_block::{dm, mount};

    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(args.is_empty() || args == ["--restore-readonly"] || args == ["--exercise-readonly"],
        "usage: pipa_mount_state_probe [--restore-readonly|--exercise-readonly]");
    ensure!(skyward_sys::prop("ro.product.device").as_deref() == Some("pipa"), "pipa only");
    let slot = skyward_vab::current_slot()?.context("no booted slot")?;
    ensure!(skyward_vab::snapshot_state()? == skyward_vab::SnapshotState::None,
        "requires no pending snapshot");
    mount::enable_source_aliases();
    // This must still work when an unmounted odm exposes the ramdisk's
    // /odm/etc -> /vendor/odm/etc -> /odm/etc compatibility-link cycle.
    println!("Readable userdata fstab entries: {}", skyward_block::fstab::read("/data")?.len());
    for base in ["system", "system_ext", "product", "odm"] {
        let name = format!("{base}{}", slot.suffix());
        let point = skyward_vab::mount_point(base);
        println!("{name}: mapped={:?}, mounted={:?}", dm::status(&name)?, mount::visible(&point)?);
        if !args.is_empty() && mount::visible(&point)?.is_none() {
            let previous = dm::status(&name)?;
            let mounted = skyward_vab::mount_one(slot, base)?;
            let current = dm::status(&name)?.context("restored mapping disappeared")?;
            ensure!(mount::visible(&mounted.mount_point)?.is_some_and(|m| m.device == current),
                "{name} did not become visible");
            ensure!(previous.is_none() || previous == Some(current),
                "{name} was unexpectedly replaced");
            println!("PASS: restored {name} read-only, existing mapping retained");
        }
        if args == ["--exercise-readonly"] {
            for entry in std::fs::read_dir("/proc")?.flatten() {
                let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else { continue };
                if std::fs::read_link(entry.path().join("exe")).ok().as_deref()
                    == Some(std::path::Path::new("/vendor/bin/cnss-daemon"))
                {
                    private_data::release_slot_mounts(pid, &[point.clone()])?;
                    println!("Released only {base}'s inherited view in cnss pid {pid}");
                }
            }
            for round in 1..=2 {
                let unmounted = skyward_vab::unmount_one(slot, base);
                // Restore the view even if its ordinary unmount or DM
                // deletion failed. Never detach the main namespace.
                let restored = skyward_vab::mount_one(slot, base);
                unmounted?;
                restored?;
                println!("PASS: {name} ordinary unmount, DM removal and read-only remount, round {round}");
            }
        }
    }
    Ok(())
}

#[cfg(not(target_os = "android"))]
fn main() {}
