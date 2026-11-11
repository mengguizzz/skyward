use anyhow::{ensure, Context, Result};
use sha2::{Digest, Sha256};
use skyward_bootimg::Entry;
use std::collections::BTreeMap;

pub const MANIFEST: &str = "skyward/boot-overlay.sha256";

#[derive(Debug, PartialEq, Eq)]
pub struct Record {
    hash: String,
    mode: u32,
    uid: u32,
    gid: u32,
}

impl Record {
    pub fn matches(&self, other: &Self) -> bool {
        // Cpio repackers normalize symlink rwx bits; targets and owners still matter.
        let symlink = self.mode & 0o170000 == 0o120000 &&
            other.mode & 0o170000 == 0o120000;
        self.hash == other.hash && self.uid == other.uid && self.gid == other.gid &&
            (self.mode == other.mode || (symlink && self.mode & !0o777 == other.mode & !0o777))
    }

    pub fn of(entry: &Entry) -> Self {
        Self {
            hash: digest(&entry.data),
            mode: entry.mode,
            uid: entry.uid,
            gid: entry.gid,
        }
    }
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn manifest<'a>(entries: impl IntoIterator<Item = &'a Entry>) -> String {
    let mut lines: Vec<_> = entries.into_iter().filter(|e| e.name != MANIFEST).map(|e| {
        let record = Record::of(e);
        format!("{} {:o} {} {} {}\n", record.hash, record.mode, record.uid, record.gid, e.name)
    }).collect();
    lines.sort();
    lines.concat()
}

pub fn parse_manifest(text: &str) -> Result<BTreeMap<String, Record>> {
    let mut records = BTreeMap::new();
    for line in text.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        ensure!(fields.len() == 5, "invalid overlay manifest row");
        ensure!(fields[0].len() == 64 && fields[0].bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid overlay digest");
        let name = fields[4];
        ensure!(!name.starts_with('/') && name != MANIFEST &&
            name.split('/').all(|s| !s.is_empty() && s != "." && s != ".."),
            "invalid overlay path");
        let record = Record {
            hash: fields[0].to_ascii_lowercase(),
            mode: u32::from_str_radix(fields[1], 8).context("invalid overlay mode")?,
            uid: fields[2].parse().context("invalid overlay uid")?,
            gid: fields[3].parse().context("invalid overlay gid")?,
        };
        ensure!(records.insert(name.into(), record).is_none(), "duplicate overlay path");
    }
    ensure!(!records.is_empty(), "empty overlay manifest");
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_duplicates_and_unsafe_paths() {
        let row = format!("{} 100755 0 0 system/bin/recovery\n", digest(b"test"));
        assert!(parse_manifest(&row).is_ok());
        assert!(parse_manifest(&(row.clone() + &row)).is_err());
        assert!(parse_manifest(&row.replace("system/bin/recovery", "../init")).is_err());
        assert!(parse_manifest("").is_err());
    }

    #[test]
    fn symlink_permission_normalization_keeps_target_type_and_ownership_checks() {
        let original = Record { hash: digest(b"/firmware"), mode: 0o120755, uid: 0, gid: 0 };
        let normalized = Record { hash: digest(b"/firmware"), mode: 0o120777, uid: 0, gid: 0 };
        assert!(original.matches(&normalized));
        assert!(!original.matches(&Record { hash: digest(b"/firmware"), mode: 0o120777, uid: 1, gid: 0 }));
        assert!(!original.matches(&Record { hash: digest(b"/other"), ..normalized }));
        let regular = Record { hash: digest(b"/firmware"), mode: 0o100755, uid: 0, gid: 0 };
        assert!(!original.matches(&regular));
        assert!(!regular.matches(&Record { hash: digest(b"/firmware"), mode: 0o100777, uid: 0, gid: 0 }));
    }
}
