//! A diagnostic compatibility profile for the tested TWRP's version binding.
use std::collections::HashMap;

const COLOROS_VENDOR: &str =
    "OPPO/OPD2515/OP6548L1:17/CP2A.260605.016/B.3b6223c-156ee02-15799c7:user/release-keys";

pub fn vendor_patch<'a>(codename: &str, props: &'a HashMap<String, String>) -> Option<&'a str> {
    if codename == "pipa"
        && props.get("ro.vendor.build.fingerprint").map(String::as_str) == Some(COLOROS_VENDOR)
        && props.get("ro.build.version.release").map(String::as_str) == Some("17")
        && props.get("ro.build.version.security_patch").map(String::as_str) == Some("2026-09-01")
        && props.get("ro.vendor.build.security_patch").map(String::as_str) == Some("2025-04-01")
    {
        return props.get("ro.build.version.security_patch").map(String::as_str);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_reported_pipa_coloros_build_uses_the_twrp_patch() {
        let props: HashMap<String, String> = [
            ("ro.vendor.build.fingerprint", COLOROS_VENDOR),
            ("ro.build.version.release", "17"),
            ("ro.build.version.security_patch", "2026-09-01"),
            ("ro.vendor.build.security_patch", "2025-04-01"),
        ].into_iter().map(|(key, value)| (key.into(), value.into())).collect();
        assert_eq!(vendor_patch("pipa", &props), Some("2026-09-01"));
        assert_eq!(vendor_patch("other", &props), None);
        for key in props.keys() {
            let mut missing = props.clone();
            missing.remove(key);
            assert_eq!(vendor_patch("pipa", &missing), None);
            let mut changed = props.clone();
            changed.insert(key.clone(), "different".into());
            assert_eq!(vendor_patch("pipa", &changed), None);
        }
    }
}
