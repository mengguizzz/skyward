# Fixed pipa Build Inputs

The pipa workflow uses this bundled file without download URLs:

- `boot-fbboot.img.gz`: gzip-compressed tested boot image. Decompression must
  produce the exact 192 MiB image recorded in `../inputs.lock.json`.

The workflow decompresses the boot image and verifies it before building.
The compressed boot is a regular Git file, not a Git LFS pointer.
Keep its bytes unchanged unless intentionally updating the locked device base.

The base supplies the device kernel and original recovery environment. It does not
freeze Skyward, its configuration, fonts, MTP rules, or cust routing. The build
creates these from the checked-out source, packs the independent boot image, then
generates `routing-data/overlay.gz` and `original-targets.txt` from that image and
the newly compiled recovery loader. No prebuilt `pipa-routing.zip` is consumed.

Push to the `pipa` branch to build and publish a pipa prerelease automatically.
The `Build Xiaomi Pad 6` workflow also supports manual runs with no inputs.
The original multi-device workflow excludes pushes to `pipa`.

These are build inputs, not release installation files. Use the generated installer
ZIP and read the release notes when installing on a device.
