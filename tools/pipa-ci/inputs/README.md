# Fixed pipa Build Inputs

The pipa workflow uses these bundled files without download URLs:

- `boot-fbboot.img.gz`: gzip-compressed tested boot image. Decompression must
  produce the exact 192 MiB image recorded in `../inputs.lock.json`.
- `pipa-routing.zip`: frozen boot routing archive recorded in the same lock file.

The workflow decompresses the boot image and verifies both inputs before building.
The compressed boot and routing archive are regular Git files, not Git LFS pointers.
Keep their bytes unchanged unless intentionally updating the locked build inputs.

Push to the `pipa` branch to build and publish a pipa prerelease automatically.
The `Build Xiaomi Pad 6` workflow also supports manual runs with no inputs.
The original multi-device workflow excludes pushes to `pipa`.

These are build inputs, not release installation files. Use the generated installer
ZIP and read the release notes when installing on a device.
