# Building and Publishing Pipa

Push the `pipa` branch to build only Xiaomi Pad 6 and publish a prerelease.
Manual runs of **Build Xiaomi Pad 6** require no inputs or URLs.

## Build Flow

1. Decompress and hash-check the device base in `inputs/boot-fbboot.img.gz`.
2. Test and compile the checked-out Skyward, static system-init, installer tools,
   and host-side `make-installer-data` generator.
3. Pack the independent boot image using the current device configuration, fonts,
   binaries and MTP rules. Verify its kernel, ramdisk and AVB digest.
4. Generate a cust boot overlay from that image and the current static loader.
   Only the recovery service route is redirected. The kernel and first-stage init
   are not included in the overlay.
5. Independently verify the overlay against the current boot image and loader.
   Exercise fresh and repeat installs on local image files with the host patcher,
   check kernel/init preservation and AVB digests, then package the installer ZIP
   with the same current Skyward binary.
6. Verify the published assets' hashes and publish with `RELEASE-NOTES.md`.

The device base and a hash-locked minimal vendor runtime are fixed binary inputs.
The runtime archive contains 47 selected service/library/configuration files,
not a complete vendor partition. The build verifies and embeds these files for
both boot and cust delivery. Launcher and compatibility code compile from source.
There is no checked-in
`pipa-routing.zip` or historical Skyward fallback.

## Local Build

Use Linux, Rust 1.99.1, the `aarch64-linux-android` target, Android NDK r29,
Python 3.11 or newer, and `lz4`. Set `ANDROID_NDK_HOME` to the Linux NDK.
From the repository root:

```sh
mkdir -p inputs
gzip -dc tools/pipa-ci/inputs/boot-fbboot.img.gz > inputs/boot-fbboot.img
bash tools/pipa-ci/build.sh inputs/boot-fbboot.img out/pipa
```

Use a fresh output directory for each build. The workflow contains the complete
toolchain setup. Publishing uses its own job's `contents: write` permission.

## Editing the Port

- Change Skyward behavior in `crates/skyward/` and its dependency crates.
- Change device settings in `experimental/pipa/device.toml`.
- Change shared ramdisk packing, properties, commands, or MTP rules in
  `crates/skyward-pack/`.
- Change cust service redirection and overlay generation in
  `tools/pipa-cust-installer/src/installer_data.rs`.
- Change the cust loader, patcher, or installation scripts under
  `tools/pipa-cust-installer/`.

Those changes are rebuilt for both delivery methods where applicable. The
standalone boot intentionally starts recovery directly; the cust installer adds
its loader route to the recovery service in the user's existing boot.

The patcher merges only the Recovery property overrides into the user's current
`prop.default`, retaining unrelated keys. Changes to other base files outside the
managed overlay are retained, and conflicting target files are refused.

## Updates and Compatibility

New installations store a hash and metadata manifest at
`skyward/boot-overlay.sha256` inside boot. On upgrade the patcher validates the
previous managed files before replacing them, then records the new hashes.
Unrelated properties may be merged even if another installer changed them.
Missing, modified, or no-longer-managed overlay files are rejected rather than
silently overwritten or abandoned.

`src/legacy-overlay.sha256` contains only file hashes and metadata for upgrading
the previously shipped frozen routing overlay. It is not used to construct new
overlays, and is not a kernel/init allowlist. Unknown third-party boot conflicts
remain errors.

The output `routing-verification.json` confirms that the overlay uses the same
recovery and loader as this build. Cloud validation does not replace device tests.

For historical logical-partition busy reports and the independent
attribute-query reproduction, see [MOUNT-BUSY.md](MOUNT-BUSY.md).
The startup investigation and partial pipa workaround are documented in
[STARTUP-MOUNTS-20261110.md](STARTUP-MOUNTS-20261110.md); they do not yet establish
that arbitrary ROM or kernel ZIPs can be installed.

The current minimal runtime, metadata compatibility workaround, in-memory
fstab snapshot, live v8 validation and remaining startup issue are documented
in [VENDOR-RUNTIME.md](VENDOR-RUNTIME.md). Local and workflow builds use the
bundled runtime automatically; `PIPA_VENDOR_RUNTIME_DIR` overrides its source
with another reviewed manifest directory. Updating Skyward does not require
regenerating this archive unless its vendor dependencies change.
