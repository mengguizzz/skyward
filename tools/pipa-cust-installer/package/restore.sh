#!/sbin/sh
set -eu
export PATH=/sbin:/system/bin:/system/xbin:/vendor/bin
cd "$(dirname "$0")"
if [ "${1:---check}" != --restore ]; then
    echo "Run in root TWRP: sh /path/to/this/restore.sh --restore"
    echo "Restores the complete saved boot and cust partitions for the recorded slot."
    exit 0
fi
[ "$(id -u)" = 0 ]
[ "$(getprop ro.product.device)" = pipa ]
# Only fixed values are accepted; never source editable metadata as shell code.
slot="$(sed -n 's/^slot=//p' device.txt)"
case "$slot" in _a|_b) ;; *) exit 1 ;; esac
boot="/dev/block/bootdevice/by-name/boot$slot"
cust=/dev/block/bootdevice/by-name/cust
sha256sum -c SHA256SUMS
[ "$(wc -c <boot.img)" = 201326592 ]
[ "$(wc -c <cust.img)" = 1073741824 ]
[ "$(blockdev --getsize64 "$boot")" = 201326592 ]
[ "$(blockdev --getsize64 "$cust")" = 1073741824 ]
while read -r source target rest; do
    case "$source" in /dev/block/*)
        if [ "$(readlink -f "$source")" = "$(readlink -f "$cust")" ]; then
            echo "Unmount cust at $target before restoring."
            exit 1
        fi
    esac
done </proc/mounts
dd if=cust.img of="$cust" bs=1048576 conv=fsync
dd if=boot.img of="$boot" bs=1048576 conv=fsync
[ "$(sha256sum "$cust" | cut -d ' ' -f1)" = "$(sed -n 's/  cust.img$//p' SHA256SUMS)" ]
[ "$(sha256sum "$boot" | cut -d ' ' -f1)" = "$(sed -n 's/  boot.img$//p' SHA256SUMS)" ]
sync
echo "Original boot$slot and cust restored. No automatic reboot."
