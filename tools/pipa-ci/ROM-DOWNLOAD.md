# Pipa official ROM downloads

Pipa uses a pinned China full Recovery ZIP rather than querying MIOTA using the
installed system's properties. Ported-ROM properties do not select the package.

Edit `crates/skyward-net/src/pipa-rom.json` to update the bundled version,
filename, HTTPS mirrors, full MD5 and exact byte length. Rebuild both the boot
image and cust installer through the normal pipa workflow. This catalog is
compiled into Skyward; it does not automatically track future Xiaomi releases.

The initial package is OS2.0.20.0.UMZCNXM, Android 14, 5,571,398,344 bytes.
Its full MD5 is `cc45265ce48adb3a6a28eb35c343a966`, recorded from matching
third-party ROM indexes, not independently recomputed from a complete download
in this session. The downloader verifies the full MD5 when the user downloads
the package. The filename's ten-character suffix is not used as the checksum.

Mirrors are cdnorg, Aliyun OSS and bigota, in that order. HEAD checks during
development returned matching sizes, ETags and CRC64 for the first two;
bigota returned HTTP 403 on the development network. A failed mirror rotates
through the existing retry loop. Pinned downloads never ask MIOTA for refreshed
URLs. Different ROM files must not share a mirror group.

Both cdnorg and OSS also passed a real `Range: bytes=0-15` request: HTTP 206,
16-byte response and `Content-Range: bytes 0-15/5571398344`. Only these small
fragments were transferred, not a complete ROM.

The page labels the package as a China official ROM, not an upgrade of a ported
Android 17 system. Reading the catalog requires no network. Downloading requires
network connectivity and unlocked internal storage. Downloads use
`/sdcard/downloaded_rom/NAME.zip.part`, support pause/resume and become
`NAME.zip` only after full digest verification. Download completion does not
start installation.

New download directories and partial files use the file manager's shared-storage
ownership, SELinux-label and permission inheritance helpers. Existing partial
files must be regular files. No recursive ownership changes are made to data,
and no Wi-Fi configuration or partition is written by catalog selection.

Validation: catalog tests and existing network/file/UI tests cover the static
package metadata and shared infrastructure. A complete 5.19 GiB device download,
pause/resume on the actual mirrors, and management of the result from Android
still require device testing. No official ROM installation is needed to test
this feature.
