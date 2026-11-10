use sha2::{Digest, Sha256};
use std::ffi::CString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::{fs::PermissionsExt, process::CommandExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

fn log(message: &str) {
    eprintln!("cust-loader: {message}");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true)
        .open("/skyward/run/cust-loader.log") {
        let _ = writeln!(file, "cust-loader: {message}");
    }
}

fn verify(root: &Path) -> Result<Vec<u8>, String> {
    let manifest = fs::read_to_string(root.join("manifest.txt")).map_err(|e| e.to_string())?;
    let mut lines = manifest.lines();
    if lines.next() != Some("pipa-v8-userspace-prototype-1") {
        return Err("payload ABI mismatch".into());
    }
    let expected = lines.next().ok_or("missing binary digest")?;
    if expected.len() != 64 || !expected.bytes().all(|c| c.is_ascii_hexdigit()) || lines.next().is_some() {
        return Err("invalid manifest".into());
    }
    let path = root.join("recovery");
    let meta = fs::metadata(&path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > 64 * 1024 * 1024 {
        return Err("invalid payload size".into());
    }
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if actual != expected.to_ascii_lowercase() {
        return Err("payload SHA-256 mismatch".into());
    }
    if bytes.get(..4) != Some(b"\x7fELF") {
        return Err("payload is not ELF".into());
    }
    Ok(bytes)
}

fn mount_cust() -> Result<PathBuf, String> {
    let device = "/dev/block/bootdevice/by-name/cust";
    let until = Instant::now() + Duration::from_secs(3);
    while !Path::new(device).exists() {
        if Instant::now() >= until {
            return Err("cust device wait timed out".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let target = "/skyward/run/cust-root";
    fs::create_dir_all(target).map_err(|e| e.to_string())?;
    let source = CString::new(device).unwrap();
    let dest = CString::new(target).unwrap();
    let kind = CString::new("ext4").unwrap();
    let opts = CString::new("noload").unwrap();
    let result = unsafe {
        libc::mount(source.as_ptr(), dest.as_ptr(), kind.as_ptr(),
            libc::MS_RDONLY | libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC,
            opts.as_ptr().cast())
    };
    if result != 0 {
        return Err(format!("cust read-only mount: {}", std::io::Error::last_os_error()));
    }
    Ok(Path::new(target).join("skyward/prototype-v1"))
}

fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_some_and(|x| x == "--verify") && args.len() == 3 {
        match verify(Path::new(&args[2])) {
            Ok(bytes) => {
                println!("verified {} bytes", bytes.len());
                return;
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(2);
            }
        }
    }
    if args.len() != 1 {
        eprintln!("usage: recovery-loader [--verify PAYLOAD_DIRECTORY]");
        std::process::exit(2);
    }
    let _ = fs::create_dir_all("/skyward/run");
    log("prototype-v1 starting; normal-boot routing is not implemented by this image");
    // The staging path permits a RAM-only test without changing cust.
    let stage = Path::new("/skyward/run/cust-test");
    let root = if stage.exists() {
        log("using RAM test payload");
        Ok(stage.to_path_buf())
    } else {
        mount_cust()
    };
    let prepared = root.and_then(|root| {
        log(&format!("checking {}", root.display()));
        let bytes = verify(&root)?;
        let target = "/skyward/run/cust-recovery";
        fs::write(target, bytes).map_err(|e| e.to_string())?;
        fs::set_permissions(target, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
        Ok(target)
    });
    match prepared {
        Ok(target) => {
            log("executing verified external userspace; argv[0]=recovery");
            let error = Command::new(target).arg0("recovery").exec();
            log(&format!("external exec failed: {error}"));
        }
        Err(error) => log(&format!("external payload unavailable: {error}")),
    }
    log("falling back to recovery built with this installer");
    let error = Command::new("/system/bin/recovery").arg0("recovery").exec();
    log(&format!("built-in exec failed: {error}"));
    std::process::exit(1);
}
