#!/usr/bin/env bash
set -euo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
if [[ $# != 2 ]]; then
    echo "Usage: bash tools/pipa-ci/build.sh BASE_BOOT OUTPUT_DIR" >&2
    exit 2
fi
base="$(realpath "$1")"
mkdir -p "$2"
out="$(realpath "$2")"
cd "$repo"
python3 tools/pipa-ci/package.py validate "$base"
python3 tools/pipa-ci/test_installer_no_data.py
: "${ANDROID_NDK_HOME:?Set ANDROID_NDK_HOME to Linux Android NDK r29}"
ndk="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$ndk/aarch64-linux-android30-clang"
export CC_aarch64_linux_android="$CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER"
export CXX_aarch64_linux_android="$ndk/aarch64-linux-android30-clang++"
export AR_aarch64_linux_android="$ndk/llvm-ar"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$repo/target}"
mkdir -p "$CARGO_TARGET_DIR"
export CARGO_TARGET_DIR="$(realpath "$CARGO_TARGET_DIR")"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
root_target="$CARGO_TARGET_DIR"
runtime_args=()
if [[ -z "${PIPA_VENDOR_RUNTIME_DIR:-}" ]]; then
    python3 tools/pipa-ci/runtime_input.py "$out/vendor-runtime"
    export PIPA_VENDOR_RUNTIME_DIR="$out/vendor-runtime"
fi
if [[ -n "${PIPA_VENDOR_RUNTIME_DIR:-}" ]]; then
    "$CC_aarch64_linux_android" -static -O2 -Wall -Wextra \
        tools/pipa-ci/vendor-runtime.c -o "$out/pipa-vendor-exec"
    "$CC_aarch64_linux_android" -shared -fPIC -O2 -Wall -Wextra \
        tools/pipa-ci/stat-compat.c -o "$out/libskyward-stat-compat.so"
    runtime_args=(--vendor-runtime "$(realpath "$PIPA_VENDOR_RUNTIME_DIR")"
        --vendor-launcher "$out/pipa-vendor-exec"
        --stat-compat "$out/libskyward-stat-compat.so")
fi
cargo test --locked --manifest-path tools/pipa-cust-installer/Cargo.toml \
    --bin boot-patcher --bin make-installer-data
cargo build --locked --release --manifest-path tools/pipa-cust-installer/Cargo.toml \
    --bin make-installer-data --bin boot-patcher
cargo test --locked -p skyward-config -p skyward-bootimg -p skyward-crypto -p skyward-net -p skyward-install
cargo build --locked --release --target aarch64-linux-android -p skyward
RUSTFLAGS="-C target-feature=+crt-static" cargo build --locked --release \
    --target aarch64-linux-android -p skyward-system-init --target-dir "$root_target/system-init"
cargo build --locked --release -p skyward-pack
RUSTFLAGS="-C target-feature=+crt-static" cargo build --locked --release \
    --manifest-path tools/pipa-cust-installer/Cargo.toml \
    --target aarch64-linux-android --target-dir "$root_target/pipa-cust-installer" \
    --bin boot-patcher --bin recovery-loader --bin cust-readonly
build_outputs() {
    local destination="$1"
    shift
    mkdir -p "$destination"
"$root_target/release/skyward-pack" recovery --experimental-boot \
    --kernel-arg cnss2.disable_nv_mac=1 --functionfs-mtp \
    --stock "$base" --binary "$root_target/aarch64-linux-android/release/skyward" \
    "${runtime_args[@]}" \
    --system-init "$root_target/system-init/aarch64-linux-android/release/system-init" \
    --config experimental/pipa/device.toml \
    --font resources/fonts/MiSansVF.ttf --mono resources/fonts/KirkMono-Regular.ttf \
    --mono-license resources/fonts/KirkMono-OFL.txt "$@" --out "$destination/skyward-pipa-boot.img"
python3 experimental/pipa/verify_image.py "$base" "$destination/skyward-pipa-boot.img" \
    --kernel-arg cnss2.disable_nv_mac=1 --functionfs-mtp \
    "${runtime_args[@]}" \
    --binary "$root_target/aarch64-linux-android/release/skyward" \
    --config experimental/pipa/device.toml "$@" --report "$destination/verification.json"
"$root_target/release/make-installer-data" "$base" "$destination/skyward-pipa-boot.img" \
    "$root_target/pipa-cust-installer/aarch64-linux-android/release/recovery-loader" "$destination/routing-data"
python3 tools/pipa-ci/package.py package "$base" "$destination/routing-data" "$root_target" "$destination"
}
# The inner installer has no embedded copy of itself; both stages use the same binary.
build_outputs "$out/reinstall-seed"
build_outputs "$out" --reinstall-zip "$out/reinstall-seed/skyward-pipa-cust-installer.zip"
python3 tools/pipa-ci/verify_retention.py "$out" "$root_target/release/boot-patcher" "$base"
