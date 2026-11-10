use anyhow::{ensure, Context, Result};
use std::ffi::CString;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    ensure!(args.len() == 2, "usage: cust-readonly MOUNT");
    let target = Path::new(&args[1]);
    let temporary = target.file_name().is_some_and(|n| n == "cust")
        && target.parent().is_some_and(|p| {
            p.parent() == Some(Path::new("/tmp"))
                && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("skyward-patch."))
        });
    ensure!(target == Path::new("/cust") || target == Path::new("/skyward/run/cust-root") || temporary,
        "unexpected cust mount path");
    ensure!(fs::canonicalize(target)? == target, "cust mount path contains a symlink");
    let cust = fs::metadata("/dev/block/bootdevice/by-name/cust")?;
    ensure!(fs::metadata(target)?.dev() == cust.rdev(), "target is not on the cust device");
    let c = CString::new(target.as_os_str().as_encoded_bytes())?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    ensure!(unsafe { libc::statvfs(c.as_ptr(), &mut st) } == 0, "statvfs failed");
    let mut flags = libc::MS_REMOUNT | libc::MS_RDONLY;
    for (has, keep) in [
        (libc::ST_NOSUID, libc::MS_NOSUID),
        (libc::ST_NODEV, libc::MS_NODEV),
        (libc::ST_NOEXEC, libc::MS_NOEXEC),
        (libc::ST_NOATIME, libc::MS_NOATIME),
        (libc::ST_NODIRATIME, libc::MS_NODIRATIME),
        (libc::ST_RELATIME, libc::MS_RELATIME),
    ] {
        if st.f_flag & has != 0 { flags |= keep; }
    }
    unsafe { libc::sync() };
    let remount = |f| unsafe {
        libc::mount(std::ptr::null(), c.as_ptr(), std::ptr::null(), f, std::ptr::null())
    };
    if remount(flags) != 0 {
        let error = std::io::Error::last_os_error();
        ensure!(error.raw_os_error() == Some(libc::EBUSY), "cust read-only remount: {error}");
        // Read-only VFS access is sufficient for the loader's retained mount.
        // Do not detach it or suppress unrelated mount errors.
        if remount(flags | libc::MS_BIND) != 0 {
            return Err(std::io::Error::last_os_error()).context("cust mount read-only remount");
        }
        println!("cust mount is read-only; filesystem-level remount was busy");
    }
    ensure!(unsafe { libc::statvfs(c.as_ptr(), &mut st) } == 0 && st.f_flag & libc::ST_RDONLY != 0,
        "cust mount is still writable");
    Ok(())
}
