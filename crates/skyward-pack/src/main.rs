use anyhow::{ensure, Context, Result};
use clap::{Parser, Subcommand};
use sha2::{Digest, Sha256};
use skyward_config::ramdisk as paths;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

mod avb;

use skyward_bootimg::{BootImage, Ramdisk};

/// Builds Skyward recovery images from a device's own stock images.
#[derive(Parser)]
#[command(name = "skyward-pack")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print an image's header and the files in its ramdisk.
    List { image: PathBuf },
    /// Write one ramdisk file to stdout.
    Cat { image: PathBuf, path: String },
    /// Unpack and pack an image's ramdisk unchanged, to check that a rebuilt
    /// archive keeps every entry.
    Repack { image: PathBuf, out: PathBuf },
    /// Join a kernel and another image's ramdisk into one boot image.
    Boot {
        /// Image holding the kernel (the device's boot partition).
        #[arg(long)]
        kernel: PathBuf,
        /// Image holding the ramdisk.
        #[arg(long)]
        ramdisk: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Build a Skyward recovery image on top of the device's stock recovery:
    /// its init, adbd, SELinux policy and libraries stay, the recovery
    /// service becomes Skyward. The image fills the whole partition and ends
    /// in an unsigned AVB footer of its own.
    Recovery {
        /// The device's stock recovery image: the whole partition, as the
        /// phone or a full update package holds it, ending in its AVB footer.
        #[arg(long)]
        stock: PathBuf,
        /// Experimental recovery-as-boot image based on an existing custom
        /// recovery. Preserve its compression and use a boot AVB descriptor.
        #[arg(long)]
        experimental_boot: bool,
        /// Append one kernel argument to an experimental boot image.
        #[arg(long, requires = "experimental_boot")]
        kernel_arg: Option<String>,
        /// Replace a custom recovery's legacy MTP actions with Skyward FunctionFS.
        #[arg(long, requires = "experimental_boot")]
        functionfs_mtp: bool,
        /// Explicitly selected pipa vendor files and their hashed manifest.
        #[arg(long, requires_all = ["experimental_boot", "vendor_launcher", "stat_compat"])]
        vendor_runtime: Option<PathBuf>,
        /// Static namespace launcher built from tools/pipa-ci/vendor-runtime.c.
        #[arg(long, requires = "vendor_runtime")]
        vendor_launcher: Option<PathBuf>,
        /// Recovery-only stat compatibility library for retained pipa kernels.
        #[arg(long, requires = "vendor_runtime")]
        stat_compat: Option<PathBuf>,
        /// Non-recursive pipa installer retained in RAM for post-flash repair.
        #[arg(long, requires = "experimental_boot")]
        reinstall_zip: Option<PathBuf>,
        /// Skyward built for the device (aarch64-linux-android).
        #[arg(long)]
        binary: PathBuf,
        /// The first program of a system installed beside the installed
        /// one, skyward-system-init built for the device and linked
        /// statically.
        #[arg(long)]
        system_init: PathBuf,
        /// The device configuration, devices/<codename>.toml.
        #[arg(long)]
        config: PathBuf,
        /// The UI typeface.
        #[arg(long)]
        font: PathBuf,
        /// The terminal's monospaced typeface.
        #[arg(long)]
        mono: PathBuf,
        /// The monospaced typeface's licence, which goes with it.
        #[arg(long)]
        mono_license: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Print the stock build a device's recovery is made on, as lines of
    /// name=value: its version, where its full update package is, and the
    /// SHA-256 of the recovery partition in that package. Given somewhere
    /// to write it, also take that partition out of the package on its
    /// server, reading only what the partition takes, and write it there
    /// once its SHA-256 is that one.
    Stock {
        /// The device configuration, devices/<codename>.toml.
        #[arg(long)]
        config: PathBuf,
        /// Where to write the stock recovery image.
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::List { image } => list(&image),
        Cmd::Cat { image, path } => {
            let rd = Ramdisk::decode(&BootImage::read(&image)?.ramdisk)?;
            let e = rd.find(&path).with_context(|| format!("{path} is not in the ramdisk"))?;
            std::io::stdout().write_all(&e.data)?;
            Ok(())
        }
        Cmd::Repack { image, out } => {
            let mut img = BootImage::read(&image)?;
            img.ramdisk = Ramdisk::decode(&img.ramdisk)?.encode()?;
            write(&out, &img, &img.to_bytes()?)
        }
        Cmd::Boot { kernel, ramdisk, out } => {
            let mut img = BootImage::read(&ramdisk)?;
            img.kernel = BootImage::read(&kernel)?.kernel;
            write(&out, &img, &img.to_bytes()?)
        }
        Cmd::Recovery { stock, experimental_boot, kernel_arg, functionfs_mtp, vendor_runtime, vendor_launcher, stat_compat, reinstall_zip, binary, system_init, config, font, mono, mono_license, out } => {
            let size = partition_size(&stock)?;
            let mut img = BootImage::read(&stock)?;
            if let Some(arg) = kernel_arg {
                append_kernel_arg(img.header.cmdline.as_mut_slice(), &arg)?;
            }
            let mut rd = Ramdisk::decode(&img.ramdisk)?;
            // A stock recovery ramdisk has no third-party tree; refuse to build
            // on top of another custom recovery.
            if !experimental_boot {
                if let Some(e) = rd.entries.iter().find(|e| e.name == "FFiles" || e.name.starts_with("twres")) {
                    anyhow::bail!("{} holds {}, which is not a stock recovery", stock.display(), e.name);
                }
            }
            let read = |p: &PathBuf| std::fs::read(p).with_context(|| format!("read {}", p.display()));
            let profile = skyward_config::DeviceConfig::load(&config)?;
            if let Some(zip) = reinstall_zip {
                ensure!(profile.device.codename == "pipa", "reinstallation is only supported on pipa");
                let bytes = read(&zip)?;
                ensure!(bytes.starts_with(b"PK\x03\x04") && bytes.len() <= 128 * 1024 * 1024,
                    "invalid reinstall ZIP");
                rd.add("skyward/reinstall/installer.sha256", 0o644,
                    format!("{:x}\n", Sha256::digest(&bytes)).into_bytes())?;
                rd.add("skyward/reinstall/installer.zip", 0o644, bytes)?;
            }
            rd.replace(paths::RECOVERY, read(&binary)?)?;
            // Install scripts look for the commands a stock recovery lacks
            // among its programs, and its unzip there too, a link to ziptool
            // until it points here; the recovery binary is each of them.
            let (_, recovery) = paths::RECOVERY.rsplit_once('/').expect("the recovery binary is in a folder");
            for command in paths::COMMANDS {
                if experimental_boot {
                    rd.entries.retain(|e| e.name != command);
                }
                rd.link(command, recovery)?;
            }
            rd.add(paths::SYSTEM_INIT, 0o755, read(&system_init)?)?;
            rd.add(paths::CONFIG, 0o644, read(&config)?)?;
            rd.add(paths::FONT, 0o644, read(&font)?)?;
            rd.add(paths::MONO, 0o644, read(&mono)?)?;
            rd.add(paths::MONO_LICENSE, 0o644, read(&mono_license)?)?;
            if let Some(runtime) = vendor_runtime {
                ensure!(profile.device.codename == "pipa", "vendor runtime is only supported on pipa");
                add_vendor_runtime(&mut rd, &runtime, &vendor_launcher.context("missing vendor launcher")?,
                    &stat_compat.context("missing stat compatibility library")?)?;
            }
            // A stock ramdisk may run logd without any scheduling profiles,
            // and logd stops as soon as it cannot find the one it asks for.
            if rd.find(paths::TASK_PROFILES).is_none() {
                rd.add(paths::TASK_PROFILES, 0o644, TASK_PROFILES.to_vec())?;
            }
            let mtp_rc = if functionfs_mtp {
                let path = "init.recovery.usb.rc";
                let original = &rd.find(path).context("custom recovery USB rules are missing")?.data;
                let updated = functionfs_usb_rules(std::str::from_utf8(original).context("USB rules are not UTF-8")?);
                rd.replace(path, updated.into_bytes())?;
                std::str::from_utf8(MTP_RC).expect("embedded init rules are UTF-8")
                    .replace("on fs && property:sys.usb.configfs=1", "on property:sys.usb.configfs=1")
                    .replace("property:sys.usb.config_disable=0", "property:sys.usb.configfs=1")
                    .into_bytes()
            } else {
                MTP_RC.to_vec()
            };
            rd.add(paths::MTP_RC, 0o644, mtp_rc)?;
            // On an unlocked device adbd keeps root when ro.secure is 0 and
            // lets any computer in without authorization when ro.adb.secure is
            // 0; init lets a later line of a property file override an earlier
            // one. update_engine refuses a package built before the recovery
            // itself unless both of the last two are set; the recovery checks
            // packages against the installed system instead.
            let stock_props = &rd
                .find(paths::PROP_DEFAULT)
                .with_context(|| format!("{} is not in the ramdisk", paths::PROP_DEFAULT))?
                .data;
            let mut props = without(stock_props, &BUILD_VERSION);
            props.extend_from_slice(b"\nro.secure=0\nro.adb.secure=0\nro.debuggable=1\nro.ota.allow_downgrade=true\n");
            if let Some(header) = profile.device.binder_header {
                props = without(&props, &["ro.skyward.binder.header"]);
                props.extend_from_slice(format!("ro.skyward.binder.header={}\n", header.as_str()).as_bytes());
            }
            rd.replace(paths::PROP_DEFAULT, props)?;
            img.ramdisk = if experimental_boot { rd.encode_like(&img.ramdisk)? } else { rd.encode()? };
            let bytes = img.to_bytes()?;
            let partition = if experimental_boot { "boot" } else { "recovery" };
            write(&out, &img, &avb::footed(&bytes, partition, size, &Sha256::digest(&bytes))?)
        }
        Cmd::Stock { config, out } => {
            let stock = skyward_config::DeviceConfig::load(&config)?.stock.with_context(|| format!("{} names no stock build", config.display()))?;
            println!("version={}\npackage={}\nrecovery={}", stock.version, stock.package, stock.recovery);
            match out {
                Some(out) => take_stock_recovery(&stock, &out),
                None => Ok(()),
            }
        }
    }
}

fn add_vendor_runtime(rd: &mut Ramdisk, root: &Path, launcher: &Path, stat_compat: &Path) -> Result<()> {
    let bytes = std::fs::read(root.join("manifest.json"))?;
    let manifest: serde_json::Value = serde_json::from_slice(&bytes)?;
    ensure!(manifest["format"] == "pipa-vendor-runtime-v1", "unknown vendor runtime format");
    let files = manifest["files"].as_array().context("runtime manifest files missing")?;
    ensure!(!files.is_empty() && files.len() <= 256, "unexpected runtime file count");
    let mut names = std::collections::BTreeSet::new();
    let mut total = 0usize;
    for file in files {
        let path = file["path"].as_str().context("runtime path missing")?;
        ensure!(!path.is_empty() && path.is_ascii()
            && path.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
            && names.insert(path.to_owned()), "invalid or duplicate runtime path");
        ensure!(path.starts_with("bin/") || path.starts_with("lib64/") || path.starts_with("etc/"),
            "unexpected runtime path: {path}");
        // Source files must be independent regular files, never links back to vendor.
        let source = root.join(path);
        let mut parent = root.to_path_buf();
        for part in path.split('/') {
            parent.push(part);
            ensure!(!std::fs::symlink_metadata(&parent)?.file_type().is_symlink(),
                "runtime source traverses a symlink: {path}");
        }
        ensure!(std::fs::symlink_metadata(&source)?.is_file(), "runtime source is not a file");
        let data = std::fs::read(source)?;
        total = total.checked_add(data.len()).context("runtime size overflow")?;
        ensure!(total <= 64 * 1024 * 1024, "runtime exceeds 64 MiB limit");
        ensure!(file["size"].as_u64() == Some(data.len() as u64)
            && file["sha256"].as_str() == Some(format!("{:x}", Sha256::digest(&data)).as_str()),
            "runtime digest or size mismatch: {path}");
        let mode = file["mode"].as_u64().context("runtime mode missing")?;
        ensure!(mode == 0o644 || mode == 0o755, "unsupported runtime mode: {path}");
        rd.add(&format!("skyward/vendor-runtime/{path}"), mode as u32, data)?;
    }
    for required in ["bin/cnss-daemon", "bin/qrtr-ns", "bin/hw/wpa_supplicant",
        "bin/hw/android.hardware.keymaster@4.0-service-qti",
        "bin/hw/android.hardware.gatekeeper@1.0-service-qti",
        "etc/wifi/WCNSS_qcom_cfg.ini", "etc/vintf/manifest/android.hardware.wifi.supplicant.xml"] {
        ensure!(names.contains(required), "required runtime file missing: {required}");
    }
    rd.add("skyward/vendor-runtime/manifest.json", 0o644, bytes)?;
    rd.link("skyward/vendor-runtime/firmware_mnt", "/firmware")?;
    rd.add("system/bin/pipa-vendor-exec", 0o755, std::fs::read(launcher)?)?;
    rd.add("system/lib64/libskyward-stat-compat.so", 0o644, std::fs::read(stat_compat)?)?;
    let path = "init.recovery.qcom.rc";
    let mut rules = String::from_utf8(rd.find(path).context("QCOM init rules missing")?.data.clone())?;
    let libraries = "    export LD_LIBRARY_PATH /system/lib64:/vendor/lib64:/vendor/lib64/hw\n";
    ensure!(rules.matches(libraries).count() == 1 && !rules.contains("export LD_PRELOAD"),
        "unexpected QCOM library environment");
    rules = rules.replacen(libraries,
        "    export LD_LIBRARY_PATH /system/lib64\n    export LD_PRELOAD /system/lib64/libskyward-stat-compat.so\n", 1);
    let trigger = "on property:vendor.sys.listeners.registered=true\n";
    ensure!(rules.matches(trigger).count() == 1, "unexpected secure service trigger");
    rules = rules.replacen(trigger,
        "on property:vendor.sys.listeners.registered=true && property:skyward.vendor-runtime.ready=1\n", 1);
    let qsee_trigger = "on init\n    start vendor.qseecomd\n";
    ensure!(rules.matches(qsee_trigger).count() == 1, "unexpected qsee startup trigger");
    rules = rules.replacen(qsee_trigger,
        "on property:skyward.vendor-runtime.ready=1\n    start vendor.qseecomd\n", 1);
    let qsee_service = "service vendor.qseecomd /vendor/bin/qseecomd\n";
    ensure!(rules.matches(qsee_service).count() == 1, "unexpected qsee service");
    rules = rules.replacen(qsee_service,
        "service vendor.qseecomd /system/bin/pipa-vendor-exec --ramdisk-security /vendor/bin/qseecomd\n", 1);
    for (service, program) in [
        ("keymaster-4-0", "/vendor/bin/hw/android.hardware.keymaster@4.0-service-qti"),
        ("gatekeeper-1-0", "/vendor/bin/hw/android.hardware.gatekeeper@1.0-service-qti"),
    ] {
        let old = format!("service {service} {program}\n");
        ensure!(rules.matches(&old).count() == 1, "unexpected {service} init route");
        rules = rules.replacen(&old, &format!("service {service} /system/bin/pipa-vendor-exec {program}\n"), 1);
    }
    rd.replace(path, rules.into_bytes())?;
    let path = "system/etc/init/hw/init.rc";
    let mut rules = String::from_utf8(rd.find(path).context("core init rules missing")?.data.clone())?;
    let libraries = "    export LD_LIBRARY_PATH /system/lib64:/vendor/lib64/hw\n";
    ensure!(rules.matches(libraries).count() == 1 && !rules.contains("export LD_PRELOAD"),
        "unexpected core library environment");
    rules = rules.replacen(libraries, "    export LD_LIBRARY_PATH /system/lib64\n", 1);
    rd.replace(path, rules.into_bytes())?;
    Ok(())
}

fn append_kernel_arg(cmdline: &mut [u8], arg: &str) -> Result<()> {
    ensure!(!arg.is_empty() && arg.is_ascii() && !arg.bytes().any(|b| b.is_ascii_whitespace() || b == 0), "expected one ASCII kernel argument");
    let end = cmdline.iter().position(|b| *b == 0).context("kernel command line has no terminator")?;
    let key = arg.split('=').next().unwrap();
    let old = std::str::from_utf8(&cmdline[..end]).context("kernel command line is not UTF-8")?;
    ensure!(!old.split_ascii_whitespace().any(|a| a.split('=').next() == Some(key)), "kernel argument {key} already exists");
    let separator = usize::from(end > 0);
    ensure!(end + separator + arg.len() < cmdline.len(), "kernel command line is full");
    if separator != 0 { cmdline[end] = b' '; }
    cmdline[end + separator..end + separator + arg.len()].copy_from_slice(arg.as_bytes());
    Ok(())
}

fn functionfs_usb_rules(original: &str) -> String {
    let mut out = String::new();
    let mut skip = false;
    for line in original.lines() {
        let text = line.trim();
        if !line.starts_with(char::is_whitespace) && !text.is_empty() && !text.starts_with('#') {
            skip = text.starts_with("on ") && text.split_ascii_whitespace().any(|word| {
                matches!(word, "property:sys.usb.config=mtp" | "property:sys.usb.config=mtp,adb")
            });
        }
        if skip { continue; }
        // Skyward owns MTP's mount and readiness; "none" must not clear descriptors it already wrote.
        if text.starts_with("mount functionfs mtp ")
            || text == "setprop sys.usb.ffs.mtp.ready 0"
            || text == "mkdir /config/usb_gadget/g1/functions/mtp.gs0"
        {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The partition a recovery is made on, as a full update package names it.
const RECOVERY: &str = "recovery";

/// Take the recovery partition out of the stock build's full update package
/// on its server, reading only what that partition takes of it, and write it
/// to `out` once its SHA-256 is the one the configuration names.
fn take_stock_recovery(stock: &skyward_config::Stock, out: &Path) -> Result<()> {
    let parent = out.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).with_context(|| format!("make {}", parent.display()))?;
    // Taken out beside where it goes, so it is moved there whole.
    let work = tempfile::Builder::new().prefix(".stock").tempdir_in(parent).with_context(|| format!("make a folder in {}", parent.display()))?;
    let payload = unpayload::Payload::open_url(&stock.package).with_context(|| format!("open {}", stock.package))?;
    let options = unpayload::ExtractOptions { output_dir: work.path().to_path_buf(), partitions: vec![RECOVERY.into()], ..Default::default() };
    let report = |event: unpayload::Event<'_>| match event {
        unpayload::Event::Started { partitions } => {
            for (name, bytes) in partitions {
                println!("taking {name} out of the package: {bytes} bytes");
            }
        }
        unpayload::Event::Extracted(image) => println!("took {} out: {} bytes", image.partition, image.size),
        unpayload::Event::Progress { .. } => {}
    };
    let taken = payload.extract(&options, &report).with_context(|| format!("take {RECOVERY} out of {}", stock.package))?;
    let image = taken.iter().find(|i| i.partition == RECOVERY).with_context(|| format!("{} holds no {RECOVERY} partition", stock.package))?;
    let sum: String = image.sha256.as_deref().context("the package's own sum of the image was not checked")?.iter().map(|b| format!("{b:02x}")).collect();
    ensure!(sum == stock.recovery, "the {RECOVERY} partition in {} has the SHA-256 {sum}, where the configuration names {}", stock.package, stock.recovery);
    std::fs::rename(&image.path, out).with_context(|| format!("move {} to {}", image.path.display(), out.display()))?;
    println!("wrote {} ({} bytes), its SHA-256 the one the configuration names", out.display(), image.size);
    Ok(())
}

/// The size of the partition `stock` fills: all of it, as the phone and a
/// full update package hold it, which its AVB footer ends.
fn partition_size(stock: &Path) -> Result<usize> {
    let mut file = File::open(stock).with_context(|| format!("open {}", stock.display()))?;
    let size = file.metadata().with_context(|| format!("stat {}", stock.display()))?.len();
    let mut magic = [0u8; 4];
    if size >= 64 {
        file.seek(SeekFrom::Start(size - 64)).with_context(|| format!("seek in {}", stock.display()))?;
        file.read_exact(&mut magic).with_context(|| format!("read {}", stock.display()))?;
    }
    ensure!(
        &magic == b"AVBf",
        "{} does not end in an AVB footer; give the whole recovery partition, as the phone or a full update package holds it",
        stock.display()
    );
    Ok(usize::try_from(size)?)
}

/// What a recovery image says about the build it is: the version and patch
/// levels of its own ramdisk's system and vendor partition, which are older
/// than those installed beside it whenever the phone has taken an update.
/// The phone's secure services read these to tell the secure world which
/// build is running, and keys the installed system made are refused when
/// they say the wrong one, so they are left out here for the recovery to
/// fill in from the installed system once it has that mounted.
const BUILD_VERSION: [&str; 3] = ["ro.build.version.release", "ro.build.version.security_patch", "ro.vendor.build.security_patch"];

/// The scheduling profiles a recovery's programs look up by name. A
/// recovery's cgroup setup mounts no controllers, so of all a system's
/// profiles do, only the timer slack applies here.
const TASK_PROFILES: &[u8] = include_bytes!("task_profiles.json");

/// The init rules that give the USB port file transfer beside adb, which a
/// stock recovery does not offer.
const MTP_RC: &[u8] = include_bytes!("mtp.rc");

/// A property file without the lines that set any of `names`.
fn without(props: &[u8], names: &[&str]) -> Vec<u8> {
    let mut out = Vec::with_capacity(props.len());
    for line in props.split_inclusive(|b| *b == b'\n') {
        let key = line.split(|b| *b == b'=').next().unwrap_or_default();
        if !names.iter().any(|n| n.as_bytes() == key) {
            out.extend_from_slice(line);
        }
    }
    out
}

/// Write `bytes`, the image `img` makes, to `out`.
fn write(out: &Path, img: &BootImage, bytes: &[u8]) -> Result<()> {
    std::fs::write(out, bytes).with_context(|| format!("write {}", out.display()))?;
    println!(
        "wrote {} ({} bytes kernel, {} bytes ramdisk, {} bytes in all)",
        out.display(),
        img.kernel.len(),
        img.ramdisk.len(),
        bytes.len()
    );
    Ok(())
}

fn list(image: &Path) -> Result<()> {
    let img = BootImage::read(image)?;
    let h = &img.header;
    println!(
        "header v{}  kernel {} bytes  ramdisk {} bytes  {:?}",
        h.header_version(),
        img.kernel.len(),
        img.ramdisk.len(),
        h.osversionpatch
    );
    if img.ramdisk.is_empty() {
        return Ok(());
    }
    let rd = Ramdisk::decode(&img.ramdisk)?;
    for e in &rd.entries {
        println!("{:06o} {:>5}:{:<5} {:>10}  {}", e.mode, e.uid, e.gid, e.data.len(), e.name);
    }
    println!("{} entries", rd.entries.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn runtime_manifest_refuses_path_escape_and_corrupt_files() {
        let root = tempfile::tempdir().unwrap();
        let mut rd = super::Ramdisk { entries: Vec::new() };
        let launcher = root.path().join("launcher");
        std::fs::write(&launcher, b"fixture").unwrap();
        let manifest = |path: &str, digest: &str| serde_json::json!({
            "format": "pipa-vendor-runtime-v1",
            "files": [{"path": path, "mode": 420, "size": 4, "sha256": digest}]
        });
        let run = |rd: &mut super::Ramdisk, value: serde_json::Value| {
            std::fs::write(root.path().join("manifest.json"), value.to_string()).unwrap();
            super::add_vendor_runtime(rd, root.path(), &launcher, &launcher).unwrap_err().to_string()
        };
        assert!(run(&mut rd, manifest("../outside", "")).contains("invalid"));
        std::fs::create_dir(root.path().join("etc")).unwrap();
        std::fs::write(root.path().join("etc/config"), b"data").unwrap();
        assert!(run(&mut rd, manifest("etc/config", &"0".repeat(64))).contains("digest"));
        assert!(rd.entries.is_empty());
        std::os::unix::fs::symlink("config", root.path().join("etc/alias")).unwrap();
        assert!(run(&mut rd, manifest("etc/alias", "")).contains("symlink"));
    }

    #[test]
    fn custom_usb_rules_keep_adb_and_fastboot_but_remove_legacy_mtp() {
        let input = "# keep license\r\non fs\r\n    mkdir /config/usb_gadget/g1/functions/mtp.gs0\r\n    mount functionfs mtp /dev/usb-ffs/mtp uid=1024\r\n    mount functionfs adb /dev/usb-ffs/adb uid=2000\r\non property:sys.usb.config=none\r\n    setprop sys.usb.ffs.mtp.ready 0\r\n    stop adbd\r\non property:sys.usb.config=mtp,adb && property:sys.usb.ffs.ready=1\r\n    symlink /config/usb_gadget/g1/functions/mtp.gs0 /config/f1\r\n    write /config/UDC controller\r\non property:sys.usb.config=fastboot\r\n    start fastbootd\r\n";
        let output = super::functionfs_usb_rules(input);
        assert!(output.contains("# keep license\n"));
        assert!(output.contains("mount functionfs adb /dev/usb-ffs/adb uid=2000"));
        assert!(output.contains("stop adbd"));
        assert!(output.contains("on property:sys.usb.config=fastboot\n    start fastbootd"));
        assert!(!output.contains("mtp"));
    }
    #[test]
    fn kernel_argument_append_preserves_existing_arguments_and_checks_capacity() {
        let mut line = [0u8; 80];
        line[..14].copy_from_slice(b"twrpfastboot=1");
        super::append_kernel_arg(&mut line, "cnss2.disable_nv_mac=1").unwrap();
        assert!(line.starts_with(b"twrpfastboot=1 cnss2.disable_nv_mac=1\0"));
        assert!(super::append_kernel_arg(&mut line, "cnss2.disable_nv_mac=0").is_err());
        assert!(super::append_kernel_arg(&mut line, "two arguments").is_err());
        assert!(super::append_kernel_arg(&mut [0u8; 3], "abc").is_err());
    }
    use super::*;

    #[test]
    fn the_lines_that_set_a_name_are_the_only_ones_left_out() {
        let props = b"ro.build.id=TQ\nro.build.version.release=13\nro.build.version.release_or_codename=13\nro.product.name=ishtar\n\
ro.vendor.build.security_patch=2026-10-01\nro.vendor.build.security_patch_level=1\n";
        let out = without(props, &BUILD_VERSION);
        let text = String::from_utf8(out).expect("property files are text");
        assert!(!text.contains("ro.build.version.release=13"));
        assert!(!text.contains("ro.vendor.build.security_patch=2026-10-01"), "the vendor partition's patch level is left out too");
        assert!(text.contains("ro.build.version.release_or_codename=13"), "a longer name is left alone");
        assert!(text.contains("ro.vendor.build.security_patch_level=1"), "a longer name is left alone");
        assert!(text.contains("ro.build.id=TQ") && text.contains("ro.product.name=ishtar"));
    }

    #[test]
    fn the_profiles_name_all_the_recovery_asks_for() {
        let profiles: serde_json::Value = serde_json::from_slice(TASK_PROFILES).expect("the profiles are JSON");
        let names = |key: &str| -> Vec<String> {
            profiles[key].as_array().expect("a list").iter().map(|p| p["Name"].as_str().expect("a name").to_string()).collect()
        };
        let (plain, aggregate) = (names("Profiles"), names("AggregateProfiles"));
        // What libprocessgroup asks for when a program sets its scheduling
        // policy, and what the recovery's own init scripts give logd.
        let asked = [
            "SCHED_SP_DEFAULT",
            "SCHED_SP_BACKGROUND",
            "SCHED_SP_FOREGROUND",
            "SCHED_SP_TOP_APP",
            "SCHED_SP_SYSTEM",
            "SCHED_SP_RT_APP",
            "CPUSET_SP_DEFAULT",
            "CPUSET_SP_BACKGROUND",
            "CPUSET_SP_FOREGROUND",
            "CPUSET_SP_TOP_APP",
            "CPUSET_SP_SYSTEM",
            "CPUSET_SP_RESTRICTED",
            "BlkIOBackground",
            "BlkIOForeground",
            "ServiceCapacityLow",
        ];
        for name in asked {
            assert!(plain.iter().chain(&aggregate).any(|n| n == name), "{name} is missing");
        }
        for a in profiles["AggregateProfiles"].as_array().expect("a list") {
            for part in a["Profiles"].as_array().expect("a list") {
                let part = part.as_str().expect("a name");
                assert!(plain.iter().any(|n| n == part), "{} is made of {part}, which is not a profile", a["Name"]);
            }
        }
    }

    #[test]
    fn the_partition_is_as_large_as_a_stock_image_ending_in_its_footer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recovery.img");
        let mut whole = vec![0u8; 8192];
        whole[8192 - 64..8192 - 60].copy_from_slice(b"AVBf");
        std::fs::write(&path, &whole).unwrap();
        assert_eq!(partition_size(&path).unwrap(), 8192);
        std::fs::write(&path, &whole[..8000]).unwrap();
        assert!(partition_size(&path).is_err(), "an image cut short of its partition's end");
        std::fs::write(&path, b"AVBf").unwrap();
        assert!(partition_size(&path).is_err(), "too short to end in a footer");
    }

    #[test]
    fn a_file_without_a_last_newline_keeps_its_last_line() {
        let out = without(b"a=1\nro.build.version.release=13", &BUILD_VERSION);
        assert_eq!(out, b"a=1\n");
        let kept = without(b"a=1\nb=2", &BUILD_VERSION);
        assert_eq!(kept, b"a=1\nb=2");
    }
}
