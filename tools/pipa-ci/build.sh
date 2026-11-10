#!/usr/bin/env bash
set -euo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
if [[ $# != 3 ]]; then
    echo "Usage: bash tools/pipa-ci/build.sh BASE_BOOT ROUTING_ZIP OUTPUT_DIR" >&2
    exit 2
fi
base="$(realpath "$1")"
routing="$(realpath "$2")"
mkdir -p "$3"
out="$(realpath "$3")"
cd "$repo"
python3 tools/pipa-ci/package.py validate "$base" "$routing"
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
cargo test --locked -p skyward-config -p skyward-bootimg -p skyward-crypto -p skyward-net -p skyward-install
cargo build --locked --release --target aarch64-linux-android -p skyward
RUSTFLAGS="-C target-feature=+crt-static" cargo build --locked --release \
    --target aarch64-linux-android -p skyward-system-init --target-dir "$root_target/system-init"
cargo build --locked --release -p skyward-pack
RUSTFLAGS="-C target-feature=+crt-static" cargo build --locked --release \
    --manifest-path tools/pipa-cust-installer/Cargo.toml \
    --target aarch64-linux-android --target-dir "$root_target/pipa-cust-installer" \
    --bin boot-patcher --bin recovery-loader --bin cust-readonly
"$root_target/release/skyward-pack" recovery --experimental-boot \
    --kernel-arg cnss2.disable_nv_mac=1 --functionfs-mtp \
    --stock "$base" --binary "$root_target/aarch64-linux-android/release/skyward" \
    --system-init "$root_target/system-init/aarch64-linux-android/release/system-init" \
    --config experimental/pipa/device.toml \
    --font resources/fonts/MiSansVF.ttf --mono resources/fonts/KirkMono-Regular.ttf \
    --mono-license resources/fonts/KirkMono-OFL.txt --out "$out/skyward-pipa-boot.img"
python3 experimental/pipa/verify_image.py "$base" "$out/skyward-pipa-boot.img" \
    --kernel-arg cnss2.disable_nv_mac=1 --functionfs-mtp \
    --binary "$root_target/aarch64-linux-android/release/skyward" \
    --config experimental/pipa/device.toml --report "$out/verification.json"
python3 tools/pipa-ci/package.py package "$base" "$routing" "$root_target" "$out"
