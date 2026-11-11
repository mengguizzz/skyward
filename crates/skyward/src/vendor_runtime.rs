use std::path::{Path, PathBuf};
use std::process::Command;

pub const ROOT: &str = "/skyward/vendor-runtime";
const LAUNCHER: &str = "/system/bin/pipa-vendor-exec";

pub fn enabled() -> bool {
    Path::new(ROOT).join("manifest.json").is_file() && Path::new(LAUNCHER).is_file()
}

pub fn routes(program: &str) -> bool {
    enabled() && matches!(program,
        "/vendor/bin/qrtr-ns" | "/vendor/bin/cnss-daemon" |
        "/vendor/bin/hw/wpa_supplicant")
}

pub fn file(path: &str) -> PathBuf {
    if enabled() {
        if let Some(relative) = path.strip_prefix("/vendor/") {
            return Path::new(ROOT).join(relative);
        }
    }
    PathBuf::from(path)
}

pub fn command(program: &str) -> Command {
    if routes(program) {
        let mut command = Command::new(LAUNCHER);
        command.arg(program);
        command
    } else {
        Command::new(program)
    }
}

pub fn isolated(pid: u32) -> std::io::Result<bool> {
    let mounts = std::fs::read_to_string(format!("/proc/{pid}/mountinfo"))?;
    Ok(mounts.lines().any(|line| {
        let fields: Vec<_> = line.split_whitespace().collect();
        fields.len() >= 6 && fields[3] == ROOT && fields[4] == "/vendor"
    }))
}
