# Pipa vendor runtime investigation (2026-11-11)

## Current device evidence

Seven processes have mappings named /vendor, but the backing device matters:

| Process | Purpose / current evidence |
| --- | --- |
| qseecomd | Initial rootfs-only snapshot was incomplete: later dlopen pulled security libraries from real vendor |
| batterysecret | Executable mapping backed by rootfs (00:02), not the mounted vendor |
| Keymaster 4.0 QTI | Vendor executable and security libraries mapped from vendor |
| Gatekeeper 1.0 QTI | Vendor executable, HAL and security libraries mapped from vendor |
| cnss-daemon | Vendor executable and QMI/WLAN libraries mapped from vendor |
| qrtr-ns | Vendor executable, libqrtr and libc++ mapped from vendor |
| wpa_supplicant | Vendor executable and Wi-Fi/security Binder libraries mapped from vendor |

This list describes observed mappings, not a complete dependency manifest.
The other five processes have observed mappings backed by real vendor (fd:03).
Rootfs mappings alone do not rule out later file opens against mounted vendor.
Dynamic dlopen, configuration, firmware requests and later feature use can
require files not present in a single maps snapshot.

Observed directory sizes, in KiB: vendor/bin 13028, vendor/lib64 357716,
vendor/etc 81156, vendor/firmware 1112, vendor/lib/modules 140.
Cust reported 576424 KiB available. These are observations, not a capacity
guarantee for an upgrade with old payload, backup and staging files.

The current kernel exposes /sys/module/wlan and no /proc/modules. This supports
the existing configuration that does not load a separate WLAN module; it does
not prove compatibility with every replacement kernel.

Cust is mounted ro,nosuid,nodev,noexec. Do not execute a copied service directly
from that mount or change it to executable as an implicit side effect.

## Experimental implementation

Include only individually justified runtime files, not a complete vendor image,
whole library directories or unrelated HALs. Start with the five services using
real vendor, recursively resolve ELF dependencies, add observed dlopen libraries,
and audit required configuration and firmware paths. Preserve the existing
ramdisk security components rather than replacing them with same-named ROM files.
Record the source, hash and inclusion reason for each selected file. Linker-only
validation is insufficient: Wi-Fi reconnects and data decryption after cold boot
must also pass before claiming the dependency set is complete.

The recovery ramdisk stores a versioned, hashed bundle at
/skyward/vendor-runtime. The kernel unpacks these independent files into RAM.
The static pipa-vendor-exec launcher gives each selected service a private
view of those files, preserving absolute /vendor paths where required.
The actual installed vendor remains a separate block device/mount managed for
installation; do not overlay the actual path in the shared namespace and
assume its references have gone away.

All references must use the independent files from process startup: executables,
libraries, open descriptors, configuration and firmware paths. A bind mount
from the real vendor is not a copy. File-backed mmap + mlock also retains the
original filesystem and does not provide this independence.

A fake vendor mount at /vendor in the main namespace would also interfere with
mount-page identity checks, fstab lookup, property lookup and third-party
updaters that expect the installed vendor there. Keep this separation explicit.

Init-managed Keymaster and Gatekeeper use the launcher through precise
recovery init service substitutions. Qseecomd also uses the launcher, in
`--ramdisk-security` mode: it detaches inherited super mounts and exposes the
original recovery ramdisk's vendor security stack, including subsequent
dlopen calls. It does not substitute the bundled ROM security libraries for
that original stack. Batterysecret retains its original ramdisk executable.
Rust-managed cnss/qrtr and the separately
spawned supplicant use the launcher too. The launcher drops inherited super
mounts in its own private namespace before execution. It retains /firmware,
which comes from the separate modem partition, and exposes it through a link
at its private /vendor/firmware_mnt. Qsee's original ramdisk vendor instead
gets a private firmware bind at that same path.

Qsee's start trigger and the secure-service property trigger also wait for
skyward.vendor-runtime.ready=1, set only after recovery has handled installed
version properties and mounted firmware. Otherwise an early private namespace
would miss the later firmware mount in the parent.

With the bundle enabled, mount modem directly at `/firmware` in the parent,
using the configured filesystem options and the fstab device resolution.
Do not create a modem child mount on the installed vendor. This keeps both
firmware and security namespaces independent of the installed vendor tree.

The real shared /vendor remains the installed filesystem. No RAM replacement
is mounted over it in the recovery/updater namespace. The old pin path skips
the routed cnss/qrtr services, so it does not accidentally pin installed files
by interpreting a private /vendor mapping in the shared namespace.
The mount page also excludes routed RAM services from its static inference
that a configured `/vendor/...` service needs the installed vendor. Actual
process descriptors and mappings are still checked; this is not a blanket
override of a busy result.

## Descriptor metadata and retained kernels

The retained device kernel also leaks a mount reference from
`newfstatat(fd, "", ..., AT_EMPTY_PATH | AT_NO_AUTOMOUNT)`. A fresh empty
tmpfs reproduced the failure: ordinary unmount failed after this call, but
succeeded after a direct `fstat` syscall or no metadata query.
Avoid assuming that Rust's `File::metadata` always invokes `fstat`: the
current toolchain issues empty-path fstatat calls, including queries inside
standard file-reading operations.

`skyward-block` now obtains the small set of path attributes needed here
through O_PATH plus explicit libc fstat. Logical mount-point lookup uses
the same helper with O_NOFOLLOW, preserving the final-symlink check.
The optional pipa runtime also includes a shared recovery-only compatibility
library, inherited through LD_PRELOAD by recovery and native services such as
adbd. It exports stat, lstat and fstatat entry points, including their 64-bit
aliases. Descriptor queries with a valid nonnegative fd and an empty path use
the direct fstat syscall. Ordinary path queries use openat(O_PATH) and fstat,
preserving dirfd and the final-symlink-following choice. The descriptor is
always closed and a failing fstat retains its errno. Empty-path AT_FDCWD uses
an O_PATH descriptor for "."; unknown flags retain newfstatat behavior.

Startup tracing also observed the dynamic linker querying real vendor library
directories before application code or preload interposition could run.
Both recovery init files therefore export only /system/lib64 as the global
library search path. The static vendor launcher restores vendor search paths
only after entering its private namespace. Explicit service-specific search
paths remain intact. All pipa CLI invocations enable the same mount-source
aliases as the full recovery, preventing diagnostic remounts from bypassing
that protection.

This is confined to the optional recovery bundle, not a kernel or Android
system change. It does not intercept direct raw syscalls. A raw statx call
also reproduced the leak, but no such production call was observed in the
captured startup trace. The v6 changes still require live vendor/odm release,
Wi-Fi and decryption validation; earlier v5 could not release live vendor.

Live v6 testing subsequently confirmed that vendor and odm were absent from
the parent mount table, all dynamic mappings were removed, and Wi-Fi connected
with Internet access. Decryption then failed before key unwrapping: the data
code reread fstab.qcom from the now-unmounted partitions and found no /data
entry. The v8 candidate retains the installed vendor/odm table text in process
memory during bring-up. Live tables take precedence while their partitions
are mounted. Unmounted partitions use that boot's snapshot, because the
ramdisk table exposed underneath vendor can omit /data. No filesystem
descriptor or mount is retained.
This snapshot is not a substitute for a bundled device-specific fstab when no
ROM is installed; that scenario remains unvalidated.

Live v8 validation: the user confirmed repeated vendor/odm mount/unmount,
Wi-Fi and data decryption. Recovery logged a successful filesystem check
(exit 0), f2fs mounted at /data, all users unlocked, and Internet reachability.
The selected services' private vendor mounts remained backed by rootfs.
No kernel oops, panic or filesystem I/O error was observed in the capture.
At capture time the user had remounted vendor/odm; their existing mappings
were therefore expected, not evidence of failed release.

One startup keystore2 registration aborted with DEAD_OBJECT/UNKNOWN_ERROR.
Init restarted it successfully about five seconds later; recovery then
deliberately restarted the registered service when publishing descriptions.
The final instance registered and completed shared-secret negotiation.
This startup sequencing issue remains open; successful decryption does not
justify calling the entire boot log error-free. Full ROM flashing, operation
with no installed ROM, and Android post-install regression testing remain
outside this validation.

The device diagnostic `pipa_mount_release_probe` exercises descriptor
metadata, empty-path queries, standard property reads, process holder scans,
ordinary unmount, inherited namespace release and actual DM removal on an
independent read-only mapping. Passing host compilation alone does not
establish that the live recovery vendor and odm are releasable.

Keep cnss's private RAM userdata isolation. Do not copy Wi-Fi calibration,
credentials, encryption keys or vendor security state into the static bundle.
Calibration and device-specific state must continue to use their intended
device-specific sources without allowing recovery writes to corrupt Android.

## First experiment

The complete vendor archive captured on the host was an isolated diagnostic
baseline only. It is not an installer payload and has not been written to cust.
The final payload must be assembled from an explicit file allowlist.

1. Inventory mapped files and ELF dependencies, then include runtime-loaded
   files and configuration observed during Wi-Fi off/on, scanning and connecting.
2. Make an independent RAM copy and exercise a non-invasive service/linker
   diagnostic in a fresh private namespace. Do not start a second live cnss or
   security daemon alongside the existing service.
3. Prepare an experimental boot with early RAM staging and explicit routes for
   all necessary services. Missing/corrupt bundle must fail clearly or use a
   documented fallback; never report vendor released while still holding it.
4. After a cold boot, inspect every process's maps, descriptors and mount
   namespaces. Ordinary unmount and actual vendor DM removal must both succeed.
5. Recheck data decryption, Wi-Fi repeated connections, MTP and charging.
   Then exercise the no-write installer preparation path.

Do not use a full ROM flash as the first validation. The isolated minimal
probe passed 84 linker checks: each selected ELF file under both library
search orders. Another 21 linker checks covered the original ramdisk security
stack in qsee's private view. These validate dependency resolution, not
cold-boot function or DM release.

## Local experimental build

Run tools/pipa-ci/build.sh normally to verify and unpack the fixed minimal
runtime archive in tools/pipa-ci/inputs. Set PIPA_VENDOR_RUNTIME_DIR to override
it with another reviewed directory containing manifest.json and its selected
files. The build compiles
the static launcher, embeds the selected files, changes the recovery init
routes, and packages the generated boot overlay into the cust installer too.
The kernel and first-stage init are retained.

The manifest format is pipa-vendor-runtime-v1, with files containing path,
size, sha256 and numeric mode (0644 or 0755). Packaging rejects path traversal,
symlinks in the input path, duplicate entries, content mismatches, missing
required services/configuration, and bundles over 64 MiB. The independent
image verifier checks embedded hashes, ownership and exact init substitutions.

The first local candidate has 47 files, totaling 12479538 bytes, excluding the
manifest and static launcher. It includes the five executables, recursive ELF
dependencies, observed runtime-loaded HALs/libraries, four WLAN configuration
files and the matching supplicant service description. These are extracted
binary inputs, not newly compiled vendor drivers.

With the bundle present, bring-up is attempted even if super cannot be
mounted. Missing installed build properties do not block Wi-Fi bring-up, but
no compatibility of encrypted data is inferred from missing version metadata.
The modem and persist partitions are still needed for firmware/calibration;
independence from system/vendor does not mean independence from every firmware
partition.

## CI and versioning

GitHub Actions does not download vendor files from a floating URL. The reviewed
47-file runtime is supplied as an explicitly locked binary build input,
separately from source, just as the existing device base is. Its manifest records
per-file hashes and inclusion reasons. It is not compiled from Skyward source.
Extraction on every recovery boot is deliberately not
used: a missing installed system must not remove the bundled functionality.

A source-derived boot/cust installer is still reproducible as installer code,
but its extracted runtime is device/system dependent. After a ROM change,
retaining the previous runtime may keep recovery usable yet cannot be assumed
compatible with the new kernel or security services. Define explicit reuse,
refresh and failure behavior before shipping.
