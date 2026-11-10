#[cfg(target_os = "android")]
fn main() -> anyhow::Result<()> {
    use skyward_install::{Event, Installer, Options, Outcome};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    let mut args = std::env::args().skip(1);
    let path = PathBuf::from(args.next().expect("ZIP path required"));
    let install = args.next().as_deref() == Some("--install");
    // The default exercises the real ZIP path without partition writes.
    unsafe { std::env::set_var("SKYWARD_INSTALL_MODE", if install { "--install" } else { "--check" }) };
    let installer = Installer::open(
        path,
        Box::new(|| anyhow::bail!("FAIL: self-update unexpectedly requested system detach")),
        Options::default(),
    )?;
    let started = Instant::now();
    loop {
        for event in installer.events() {
            println!("{event:?}");
            match event {
                Event::Script => installer.install(),
                Event::Finished(Outcome::Ran { wipe_cache: false }) => {
                    println!("PASS: real installer completed without detaching system partitions");
                    return Ok(());
                }
                Event::Finished(outcome) => anyhow::bail!("installer ended: {outcome:?}"),
                _ => {}
            }
        }
        if !install && started.elapsed() > Duration::from_secs(120) {
            installer.cancel()?;
            anyhow::bail!("compatibility check timed out");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(not(target_os = "android"))]
fn main() {}
