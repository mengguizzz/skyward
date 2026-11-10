use std::collections::BTreeSet;
use std::ffi::CString;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Release inherited slot mounts in cnss's namespace without joining it
/// in the recovery process or exposing real userdata to the daemon.
pub fn release_slot_mounts(pid: u32, points: &[PathBuf]) -> io::Result<()> {
    let namespace = std::fs::File::open(format!("/proc/{pid}/ns/mnt"))?;
    if std::fs::read_link(format!("/proc/{pid}/ns/mnt"))?
        == std::fs::read_link("/proc/self/ns/mnt")?
    {
        return Err(io::Error::other("cnss must have a private mount namespace"));
    }
    let mut points = points.to_vec();
    points.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    let points: Vec<CString> = points.iter()
        .map(|p| CString::new(p.as_os_str().as_bytes()))
        .collect::<Result<_, _>>()?;
    let mut child = Command::new("/system/bin/true");
    child.current_dir("/").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    unsafe {
        child.pre_exec(move || {
            if libc::setns(namespace.as_raw_fd(), libc::CLONE_NEWNS) != 0 {
                return Err(io::Error::last_os_error());
            }
            // cnss owns a private namespace. Lazy detach drops only these
            // inherited mount references; pin() keeps its active files alive.
            for point in &points {
                if libc::umount2(point.as_ptr(), libc::MNT_DETACH) != 0 {
                    let err = io::Error::last_os_error();
                    if !matches!(err.raw_os_error(), Some(libc::EINVAL) | Some(libc::ENOENT)) {
                        return Err(err);
                    }
                }
            }
            Ok(())
        });
    }
    let status = child.status()?;
    if !status.success() {
        return Err(io::Error::other(format!("release cnss slot mounts: {status}")));
    }
    Ok(())
}

/// Prepared before fork; entering the namespace uses only system calls.
pub struct PrivateData {
    detach: Vec<CString>,
}

impl PrivateData {
    pub fn prepare() -> io::Result<Self> {
        let text = std::fs::read_to_string("/proc/self/mountinfo")?;
        let rows: Vec<Vec<&str>> = text.lines().map(|s| s.split_whitespace().collect()).collect();
        if rows.iter().any(|r| r.len() < 7) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid mountinfo"));
        }
        let devices: BTreeSet<_> = rows.iter().filter(|r| r[4] == "/data").map(|r| r[2]).collect();
        let mut ids: BTreeSet<_> = rows.iter().filter(|r| {
            r[4] == "/data" || r[4].starts_with("/data/") || devices.contains(r[2])
                && r[4] != "/"
        }).map(|r| r[0]).collect();
        // Include FUSE/bind children, even when they have another device number.
        loop {
            let before = ids.len();
            for r in &rows {
                if ids.contains(r[1]) {
                    ids.insert(r[0]);
                }
            }
            if ids.len() == before { break; }
        }
        let mut detach = Vec::new();
        for r in rows.iter().rev().filter(|r| ids.contains(r[0])) {
            let mut bytes = Vec::new();
            let raw = r[4].as_bytes();
            let mut i = 0;
            while i < raw.len() {
                if raw[i] == b'\\' {
                    if i + 3 >= raw.len() || !raw[i + 1..i + 4].iter().all(|b| (b'0'..=b'7').contains(b)) {
                        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid mount escape"));
                    }
                    bytes.push((raw[i + 1] - b'0') * 64 + (raw[i + 2] - b'0') * 8 + raw[i + 3] - b'0');
                    i += 4;
                } else {
                    bytes.push(raw[i]);
                    i += 1;
                }
            }
            detach.push(CString::new(bytes)?);
        }
        Ok(Self { detach })
    }

    pub fn enter(&self) -> io::Result<()> {
        let check = |rc| if rc == 0 { Ok(()) } else { Err(io::Error::last_os_error()) };
        unsafe {
            check(libc::unshare(libc::CLONE_NEWNS))?;
            check(libc::mount(std::ptr::null(), c"/".as_ptr(), std::ptr::null(),
                libc::MS_REC | libc::MS_PRIVATE, std::ptr::null()))?;
            for path in &self.detach {
                let rc = libc::umount2(path.as_ptr(), libc::MNT_DETACH);
                if rc != 0 {
                    let err = io::Error::last_os_error();
                    if !matches!(err.raw_os_error(), Some(libc::EINVAL) | Some(libc::ENOENT)) {
                        return Err(err);
                    }
                }
            }
            check(libc::mount(c"tmpfs".as_ptr(), c"/data".as_ptr(), c"tmpfs".as_ptr(),
                libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC,
                c"size=16m,mode=0755".as_ptr().cast()))?;
            for path in [c"/data/vendor", c"/data/vendor/wifi", c"/data/vendor/wifi/sockets"] {
                check(libc::mkdir(path.as_ptr(), 0o700))?;
            }
        }
        Ok(())
    }
}
