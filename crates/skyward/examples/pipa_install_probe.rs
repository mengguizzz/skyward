#[cfg(target_os = "android")]
#[path = "../src/services.rs"]
mod services;

#[cfg(target_os = "android")]
fn main() -> anyhow::Result<()> {
    use anyhow::{ensure, Context};
    use skyward_install::{Event, Installer, Options, Outcome};
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    ensure!(skyward_sys::prop("ro.product.device").as_deref() == Some("pipa"), "pipa only");
    let path = PathBuf::from("/tmp/skyward-slot-probe.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path)?);
    zip.start_file("META-INF/com/google/android/update-binary", zip::write::SimpleFileOptions::default())?;
    zip.write_all(b"#!/system/bin/sh\n\
        if grep -E ' /(odm|vendor|mnt/system|mnt/product|mnt/system_ext) ' /proc/mounts; then exit 2; fi\n\
        for name in system system_ext product vendor odm; do\n\
          test -b /dev/block/mapper/${name}$(getprop ro.boot.slot_suffix) || exit 3\n\
        done\n\
        echo 'PASS: ordinary ZIP reached script with slot mounts released'\n\
        exit 0\n")?;
    zip.finish()?;
    let services = skyward_config::DeviceConfig::load("/skyward/device.toml")?.services;
    let installer = Installer::open(path, Box::new(move || {
        let slot = skyward_vab::current_slot()?.context("no slot")?;
        let mounted = skyward_vab::mounted_partitions(slot)?;
        let busy = services::pin(&services, &mounted)?;
        skyward_vab::release_partitions(slot, &busy)
    }), Options { keep: false, signed: false })?;
    let started = Instant::now();
    loop {
        for event in installer.events() {
            println!("{event:?}");
            match event {
                Event::Script => installer.install(),
                Event::Finished(Outcome::Ran { wipe_cache: false }) => {
                    let log = std::fs::read_to_string(skyward_install::SCRIPT_LOG)?;
                    print!("{log}");
                    ensure!(log.contains("PASS: ordinary ZIP reached script"), "probe script did not pass");
                    println!("PASS: real ordinary ZIP install pipeline completed; probe writes no partitions");
                    return Ok(());
                }
                Event::Finished(outcome) => anyhow::bail!("probe ended: {outcome:?}"),
                _ => {}
            }
        }
        if started.elapsed() > Duration::from_secs(120) {
            installer.cancel()?;
            anyhow::bail!("probe timed out");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(not(target_os = "android"))]
fn main() {}
