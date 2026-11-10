#!/sbin/sh
set -eu
export PATH=/sbin:/system/bin:/system/xbin:/vendor/bin
here="$(cd "$(dirname "$0")" && pwd)"
cd "$here"
ui() {
    echo "$*"
    if [ -n "${OUTFD:-}" ]; then
        printf '%s\n' "ui_print $*" "ui_print" >>"/proc/self/fd/$OUTFD"
    fi
}
fail() { ui "ERROR: $*"; exit 1; }
mode="${1:---check}"
case "$mode" in --check|--install) ;; *) fail "Use --check or --install" ;; esac
[ "$(id -u)" = 0 ] || fail "Root recovery required"
[ "$(getprop ro.product.device)" = pipa ] || fail "Only Xiaomi Pad 6 pipa is supported"
[ "$(getenforce)" = Permissive ] || fail "This build requires permissive recovery"
slot="$(getprop ro.boot.slot_suffix)"
case "$slot" in _a|_b) ;; *) fail "Cannot determine current slot" ;; esac
boot="/dev/block/bootdevice/by-name/boot$slot"
cust=/dev/block/bootdevice/by-name/cust
[ "$(blockdev --getsize64 "$boot")" = 201326592 ] || fail "Unexpected boot size"
[ "$(blockdev --getsize64 "$cust")" = 1073741824 ] || fail "Unexpected cust size"
sha256sum -c SHA256SUMS >"$here/package-verification.log" || fail "Package integrity check failed"
ui "Skyward pipa cust installer v8"
ui "Target: boot$slot only; kernel retained; cust contents preserved"
work="$(mktemp -d /tmp/skyward-patch.XXXXXX)"
cust_mount="$work/cust"
mkdir "$cust_mount"
mounted=0
boot_touched=0
cust_touched=0
committed=0
backup=
prior_cust_ro=0
using_existing_cust=0
cust_mount_rw=0
had_skyward=0
existing_cust_mount=
publish_backup() {
    [ -n "$backup" ] && [ -d "$backup" ] || return 0
    case "$backup" in /data/media/0/Skyward-backups/*) ;; *) return 1 ;; esac
    [ "$(readlink -f "$backup")" = "$backup" ] || return 1
    chown 1023:1023 /data/media/0/Skyward-backups || return 1
    chmod 2770 /data/media/0/Skyward-backups || return 1
    find "$backup" -type d -exec chown 1023:1023 {} + || return 1
    find "$backup" -type d -exec chmod 2770 {} + || return 1
    find "$backup" -type f -exec chown 1023:1023 {} + || return 1
    find "$backup" -type f -exec chmod 660 {} + || return 1
    if [ -f "$backup/restore.sh" ]; then chmod 770 "$backup/restore.sh" || return 1; fi
}
make_cust_writable() {
    if [ "$using_existing_cust" = 1 ]; then
        mount -o remount,rw "$cust_mount" || return 1
    else
        if [ "$mounted" = 1 ]; then
            umount "$cust_mount" || return 1
            mounted=0
        fi
        mount -t ext4 -o rw,nosuid,nodev,noexec "$cust" "$cust_mount" || return 1
        mounted=1
    fi
    cust_mount_rw=1
}
remove_new_payload() {
    make_cust_writable || return 1
    clear_skyward || return 1
    if [ "$had_skyward" = 1 ]; then
        tar -xpf "$backup/skyward-files.tar" -C "$cust_mount" || return 1
    fi
    sync
}
clear_skyward() {
    # Restrict recursive deletion to the dedicated directory on the verified cust mount.
    case "$cust_mount" in /cust|/skyward/run/cust-root|/tmp/skyward-patch.*/cust) ;; *) return 1 ;; esac
    [ "$(readlink -f "$cust_mount")" = "$cust_mount" ] || return 1
    [ ! -L "$cust_mount/skyward" ] || return 1
    rm -rf "$cust_mount/skyward"
}
finish() {
    result=$?
    trap - EXIT
    if [ "$committed" = 0 ] && [ -n "$backup" ]; then
        if [ "$boot_touched" = 1 ]; then
            ui "Restoring original boot after failure..."
            dd if="$backup/boot.img" of="$boot" bs=1048576 conv=fsync || result=1
            [ "$(sha256sum "$boot" | cut -d ' ' -f1)" = "$boot_hash" ] || result=1
        fi
        if [ "$cust_touched" = 1 ]; then
            ui "Restoring the previous Skyward directory after failure..."
            if ! remove_new_payload; then
                ui "ERROR: payload cleanup failed; full cust backup retained in $backup"
                result=1
            fi
        fi
    fi
    if [ "$using_existing_cust" = 1 ] && [ "$cust_mount_rw" = 1 ]; then
        "$here/tools/cust-readonly" "$cust_mount" || result=1
    fi
    if [ "$mounted" = 1 ]; then
        umount "$cust_mount" || result=1
    fi
    if ! publish_backup; then
        ui "WARNING: backup permissions could not be normalized: $backup"
    fi
    rm -rf "$work"
    exit "$result"
}
trap finish EXIT
ui "Reading current boot and checking compatibility..."
dd if="$boot" of="$work/original.img" bs=1048576 status=none
"$here/tools/boot-patcher" "$work/original.img" "$here/data" "$work/patched.img" >"$work/patch.log" 2>&1 ||
    { cat "$work/patch.log"; fail "Unsupported or conflicting boot; no partitions changed"; }
cat "$work/patch.log"
if [ "$mode" = --check ]; then
    ui "Compatibility check passed. No partitions changed."
    exit 0
fi
# Reuse the mount owned by TWRP or the cust loader; do not unmount it.
real_cust="$(readlink -f "$cust")"
while read -r source target filesystem options rest; do
    case "$source" in /dev/block/*)
        if [ "$(readlink -f "$source")" = "$real_cust" ]; then
            case "$target" in /cust|/skyward/run/cust-root) ;; *)
                fail "Unsupported cust mount: $target" ;;
            esac
            [ -z "$existing_cust_mount" ] || fail "Multiple cust mounts are present"
            existing_cust_mount="$target"
            case "$options" in ro|ro,*) prior_cust_ro=1 ;;
                *) fail "cust must be read-only before installation" ;;
            esac
        fi
    esac
done </proc/mounts
[ -d /data/media/0 ] && [ -w /data/media/0 ] || fail "Decrypt/mount data in recovery first"
available="$(df -Pk /data/media/0 | tail -n 1 | awk '{print $4}')"
case "$available" in ''|*[!0-9]*) fail "Cannot read data free space" ;; esac
[ "$available" -ge 1400000 ] || fail "At least 1.4 GB free data space is required for backups"
backup="/data/media/0/Skyward-backups/$(date +%Y%m%d-%H%M%S)-slot${slot}-$$"
mkdir -p "$backup"
publish_backup || fail "Cannot prepare backup directory permissions"
cp "$work/original.img" "$backup/boot.img"
ui "Backing up the complete cust partition; please wait..."
dd if="$cust" of="$backup/cust.img" bs=1048576 conv=fsync status=none
[ "$(wc -c <"$backup/boot.img")" = 201326592 ] || fail "Boot backup truncated"
[ "$(wc -c <"$backup/cust.img")" = 1073741824 ] || fail "cust backup truncated"
boot_hash="$(sha256sum "$work/original.img" | cut -d ' ' -f1)"
cust_hash="$(sha256sum "$cust" | cut -d ' ' -f1)"
[ "$(sha256sum "$backup/boot.img" | cut -d ' ' -f1)" = "$boot_hash" ] || fail "Boot backup mismatch"
[ "$(sha256sum "$backup/cust.img" | cut -d ' ' -f1)" = "$cust_hash" ] || fail "cust backup mismatch"
printf '%s  boot.img\n%s  cust.img\n' "$boot_hash" "$cust_hash" >"$backup/SHA256SUMS"
printf 'device=pipa\nslot=%s\nboot=%s\ncust=%s\n' "$slot" "$boot" "$cust" >"$backup/device.txt"
cp "$here/restore.sh" "$backup/restore.sh"
cp "$work/patch.log" "$backup/patch.log"
sync
if [ "$prior_cust_ro" = 1 ]; then
    cust_mount="$existing_cust_mount"
    using_existing_cust=1
    ui "Using recovery's existing read-only cust mount: $cust_mount"
else
    mount -t ext4 -o ro,noload,nosuid,nodev,noexec "$cust" "$cust_mount"
    mounted=1
fi
# Record all existing regular files, excluding the installer's own namespace.
find "$cust_mount" -path "$cust_mount/skyward" -prune -o -type f -exec sha256sum {} \; >"$work/cust-files.sha256"
if [ -e "$cust_mount/skyward" ]; then
    [ -d "$cust_mount/skyward" ] && [ ! -L "$cust_mount/skyward" ] ||
        fail "Skyward namespace is not a normal directory"
    ui "Backing up the existing Skyward directory before replacement..."
    tar -cpf "$backup/skyward-files.tar" -C "$cust_mount" skyward
    tar -tf "$backup/skyward-files.tar" >"$backup/skyward-files-list.txt"
    had_skyward=1
fi
"$here/tools/recovery-loader" --verify "$here/payload"
make_cust_writable || fail "Cannot make cust writable"
cust_touched=1
ui "Removing the previous Skyward directory and installing a fresh payload..."
clear_skyward || fail "Cannot safely clear the Skyward directory"
mkdir "$cust_mount/skyward"
mkdir "$cust_mount/skyward/.prototype-v1-installing"
cp "$here/payload/recovery" "$cust_mount/skyward/.prototype-v1-installing/recovery"
cp "$here/payload/manifest.txt" "$cust_mount/skyward/.prototype-v1-installing/manifest.txt"
chown -R 0:0 "$cust_mount/skyward"
chmod 755 "$cust_mount/skyward" "$cust_mount/skyward/.prototype-v1-installing"
chmod 644 "$cust_mount/skyward/.prototype-v1-installing/"*
"$here/tools/recovery-loader" --verify "$cust_mount/skyward/.prototype-v1-installing"
mv "$cust_mount/skyward/.prototype-v1-installing" "$cust_mount/skyward/prototype-v1"
sha256sum -c "$work/cust-files.sha256" >"$backup/cust-files-check.log" || fail "Existing cust files changed"
sync
if [ "$using_existing_cust" = 1 ]; then
    if [ "$cust_mount_rw" = 1 ]; then
        "$here/tools/cust-readonly" "$cust_mount"
        cust_mount_rw=0
    fi
else
    umount "$cust_mount"
    mounted=0
fi
patched_hash="$(sha256sum "$work/patched.img" | cut -d ' ' -f1)"
if [ "$patched_hash" = "$boot_hash" ]; then
    ui "boot$slot already has the required patch; no boot write needed."
else
    ui "Writing patched boot$slot..."
    boot_touched=1
    dd if="$work/patched.img" of="$boot" bs=1048576 conv=fsync status=none
fi
[ "$(sha256sum "$boot" | cut -d ' ' -f1)" = "$patched_hash" ] || fail "Boot readback mismatch"
printf '%s\n' "$patched_hash" >"$backup/installed-boot.sha256"
sync
committed=1
ui "Installation complete. No automatic reboot."
ui "Backups: $backup"
ui "Normal boot uses Android; recovery loads Skyward from cust."
