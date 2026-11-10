#[cfg(target_os = "android")]
#[path = "../src/private_data.rs"]
mod private_data;

#[cfg(target_os = "android")]
fn main() -> std::io::Result<()> {
    let mounts = std::fs::read_to_string("/proc/self/mountinfo")?;
    let devices: Vec<String> = mounts.lines().filter_map(|s| {
        let row: Vec<_> = s.split_whitespace().collect();
        (row[4] == "/data").then(|| row[2].to_owned())
    }).collect();
    let before = std::fs::read_link("/proc/self/ns/mnt")?;
    private_data::PrivateData::prepare()?.enter()?;
    assert_ne!(before, std::fs::read_link("/proc/self/ns/mnt")?);
    std::fs::write("/data/vendor/wifi/sockets/skyward-isolation-probe", b"RAM only\n")?;
    let mounts = std::fs::read_to_string("/proc/self/mountinfo")?;
    assert!(!mounts.lines().any(|s| devices.iter().any(|d| s.split_whitespace().nth(2) == Some(d.as_str()))));
    println!("No inherited userdata mounts or aliases remain.");
    let data = mounts.lines().find(|s| s.split_whitespace().nth(4) == Some("/data")).unwrap();
    assert!(data.contains(" - tmpfs "));
    println!("private namespace: {:?}", std::fs::read_link("/proc/self/ns/mnt")?);
    println!("private data: {data}");
    println!("private socket directory write succeeded");
    Ok(())
}

#[cfg(not(target_os = "android"))]
fn main() {
    eprintln!("This diagnostic requires Android.");
}
