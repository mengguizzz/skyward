# Pipa ColorOS decryption experiment

Status: on 2026-11-13, the user reported successful ColorOS decryption with
the v2 test package and also relayed successful decryption on an AOSP-like ROM.
The latter ROM's exact build and detailed unlock coverage were not supplied.

The reported ColorOS17-Port-pipa-beta7.1-20261105 previously failed while unwrapping
`/metadata/vold/metadata_encryption/key`: Keymaster returns -62, then -38
during key upgrade. The saved boot-fbboot TWRP can decrypt according to the
tester. Its static vendor patch property is 2026-09-01; Skyward previously
adopted the installed vendor's 2025-04-01.

This experiment makes two changes:

- KeyMint GCM key upgrade carries only the application binding. The nonce,
  block mode, padding and MAC length remain parameters of the decryption
  operation, not the blob upgrade.
- Only pipa with the exact reported OPPO vendor fingerprint, Android 17,
  system patch 2026-09-01 and vendor patch 2025-04-01 uses the saved TWRP's
  vendor patch value when initializing security services. All other builds
  retain the existing installed-property behavior.

The profile is in `crates/skyward/src/pipa_keymaster.rs`. A kernel-log message
identifies activation and records both the installed and selected values.
This does not change the ROM property files or persist renewed key blobs.
It does not prove that the static TWRP value matches its runtime value.

The two changes were tested together, so the successful feedback does not
establish which change resolved the original failure. It also does not imply
support for every ColorOS or AOSP-like build. For future failures, capture
profile activation and distinguish metadata, device-encrypted data and
credential-encrypted user data. A dedicated HyperOS regression result for
this package has not been recorded.

Installer v9 no longer requires writable/decrypted data or creates persistent
boot/cust images. It only prints a manual-backup notice. Original boot bytes
already read for patching and the previous Skyward directory are retained in
private RAM scratch space for failure rollback, then removed on success.
It does not delete backups made by previous installers.
