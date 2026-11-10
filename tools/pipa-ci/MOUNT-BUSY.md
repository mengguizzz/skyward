# Pipa Mount Busy Investigation

Status: investigated on November 10, 2026. Arbitrary kernel/ROM ZIP installation
is not yet verified or fixed. No kernel source or installed kernel was changed.

Follow-up: [STARTUP-MOUNTS-20261110.md](STARTUP-MOUNTS-20261110.md) records the
startup timeline, init mount-observer reproduction, opt-in pipa source-alias
workaround, and successful removal of fresh read-only probe mappings.
The complete recovery startup and arbitrary updater workflow remain unverified.

## Installation Failure

The actual recovery log shows cnss namespace cleanup completing, followed by:

```text
move off the installed system: unmount /odm:
Device or resource busy (os error 16)
```

The third-party ZIP updater has not run at this point. The cnss namespace no
longer contained the slot mounts. Process scans found no open descriptors,
mappings, working/root directories, or nested mounts explaining the `/odm`
reference. Persist and metadata also failed to unmount during early startup,
before the slot filesystems and cnss were started.

## Independent Reproduction

The running kernel is `4.19.404R #27`, built November 2, 2026.
A small static NDK executable reproduces the following with fresh empty tmpfs
mounts in a recursively private mount namespace, without using super, cust,
userdata, Skyward services, or a ZIP updater:

| Operation before ordinary umount | Result |
| --- | --- |
| No operation | OK |
| `stat(path)` | EBUSY |
| Direct `newfstatat` system call | EBUSY |
| Child calls `newfstatat`, parent waits for child exit | EBUSY |
| `open(O_PATH)` + direct `fstat` + close | OK |
| `access(path, F_OK)` | OK |

The direct-syscall case excludes libc's wrapper as a prerequisite. The child-exit
case excludes an ordinary descriptor remaining open in that child. This
establishes an attribute-query reference problem in the current running kernel
environment, not the cause of every historical busy error.

The locally available `/root/kernel_e404_hos4/fs/stat.c` has a matching source
defect: `vfs_statx` puts `path_put(&path)` inside the `#else` for
`CONFIG_KSU_SUSFS_SUS_MOUNT`. With the option enabled, successful path lookup
references are not released. The exact source revision of the running #27
binary was not matched; this is supporting source evidence, not a verified
binary-to-source match.

Compile the adjacent diagnostic using NDK r29:

```sh
"$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android30-clang" \
  -static -O2 -Wall -Wextra -Werror \
  tools/pipa-ci/probe-mount-busy.c -o /path/on/F/probe-mount-busy
adb push /path/on/F/probe-mount-busy /tmp/probe-mount-busy
adb shell chmod 755 /tmp/probe-mount-busy
adb shell /tmp/probe-mount-busy
```

Run as root in recovery. Exit code 0 means all normal unmounts succeeded,
2 means at least one returned EBUSY, and 1 means a diagnostic operation failed.
The helper detaches only its own empty RAM mount if normal cleanup fails.
It never detaches inherited device mounts or writes device partitions.

## Userspace Experiment and Limits

A temporary LD_PRELOAD wrapper replaced path-stat calls with `openat(O_PATH)` +
`fstat` + close. The dynamic reproduction program then unmounted normally.
The same preload did not fix the static reproduction program.

This experiment is not integrated into init, boot, cust, or the release workflow:

- Already leaked references cannot be closed by adding a wrapper afterward.
- Static updater tools and direct system calls bypass LD_PRELOAD.
- Every process accessing affected mounts needs suitable coverage.
- Legitimate live service/library references still need to be released.

Global lazy unmount is not a complete fix: invisible mounts can still retain
their filesystem and device-mapper devices. Do not ignore unmap errors or clear
OTA state to conceal them.

The existing cnss namespace cleanup addresses a separate confirmed reference
source. Keep it. Its earlier probe passed in that environment, not universally.
`services::pin()` also deliberately retains file-backed mmap/mlock references
for vendor services. Removing/recreating all logical mappings must account for
those services too. Fixing attribute queries alone does not prove ROM flashing.

The standalone image's base contains `4.19.325-cip136-st20-N0-kernel++ #8`.
The cust installer preserves the current kernel. Different version strings do
not establish which works; a standalone-boot comparison was not performed here.
Do not reinstate a kernel hash allowlist based on these findings.

## Fastbootd Status

Read-only checks in recovery showed BootControl merge status `none`, Skyward
standing `None`, and libsnapshot state `None`. After rebooting to fastbootd:

```text
snapshot-update-status: none
is-userspace: yes
current-slot: b
```

FastbootEnhance upstream reads `snapshot-update-status`; its warning treats a
non-null value other than `none` as an update in progress. The reported warning
was not reproduced during these queries. Pending OTA metadata has not been
established as the cause of busy.

Separately, `fastboot getvar all` failed with `Unable to query battery data`.
Individual status/slot queries succeeded. Investigate fastbootd/HAL information
retrieval separately if the UI cannot refresh its information.

The device was returned to recovery. No snapshot cancellation, boot-control
reset, firmware ZIP execution, or boot/cust/super patching was performed.
