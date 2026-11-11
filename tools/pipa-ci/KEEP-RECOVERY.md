# Pipa recovery retention after installing a package

Status: RAM installer implementation, 2026-11-11. Kernel-package automatic
retention and recovery re-entry passed on hardware. Post-reboot logs confirm
data decryption, Wi-Fi scanning and MTP session setup; Wi-Fi Internet and
complete user workflow validation remain separate checks.

## Implemented Scope

Pipa exposes the existing retention switch through a separate `boot_reinstall`
capability. Recovery-partition and dual-boot detection remain separate.
The switch applies to successful payload OTA and script ZIP installs. Direct
partition-image writes do not run this backend. The cust self-installer keeps
its existing independent path and does not invoke nested retention.

The build first generates a complete seed installer whose overlay has no
embedded installer. It then builds the final boot and outer installer with that
seed ZIP and its digest at `/skyward/reinstall/`. Both use the same compiled
Skyward binary. The seed is not a frozen historical download and is regenerated
on every build. Adding the seed ZIP to the final overlay avoids self-recursion.

Before a package runs, the selected backend verifies the seed digest, extracts
it into private `/tmp/skyward-retain-*` staging and records both boot hashes.
It depends on neither Android data nor a cust mount to read the staged package.
After installation it selects the next boot slot and any other changed boot.
Each selected target passes `install.sh --retain-check SLOT` before any target
is written, then `--retain SLOT` performs boot repair with readback verification.

Both modes reuse the ordinary boot patcher's device/layout/conflict checks.
They explicitly name the target slot rather than relying on the booted-slot
property. The patcher also reinstalls the seed archive and digest, including
them in the managed overlay stamp, so repeated retention remains available.

Automatic retention requires the cust payload to verify and match this build.
It opens cust read-only, never replaces its payload and never formats it.
If a ROM replaced cust or installed an unsupported boot, retention fails
explicitly; this initial implementation does not attempt full environment
reconstruction or automatic cust restoration.

No full cust backup or writable data is required for retention. Each boot is
backed up temporarily in RAM immediately before it is written. On a write or
readback failure the script attempts to restore that post-ROM boot. This is not
a persistent backup or protection against power loss. Slot writes are
sequential; a failure on a later slot does not undo successful earlier slots.

Failed script installs skip pipa retention. Retention errors are reported
separately from successful ROM installation, logged at
`/skyward/run/reinstall.log`, and inhibit automatic completion reboot.
They do not make a failed ROM install bootable.

Host validation includes slot selection, hash-corruption rejection, staging
surviving source removal, the UI capability, and repeat managed-overlay repair.
`verify_retention.py` verifies the actual generated nested ZIP, absence of
self-recursion, retention of a changed synthetic kernel and normal arguments,
idempotence, AVB digest and unsupported-input refusal. These are image-file
tests, not proof of a completed real ROM flash.

## Validation Record (2026-11-11)

The first real kernel-package test returned success from AnyKernel but retention
stopped before patching: the manifest declared AIDL boot control without a
registered instance. The device's bootctl triggered lazy startup of that same
AIDL service and then read active slot B; the original Skyward slot diagnostic
also worked afterward. Pipa retention now requests the AIDL interface through
init on lookup failure and waits up to five seconds for registration, without
guessing a slot. Target-discovery errors are also recorded in the retention log.
The subsequent kernel-package test below exercised this fix successfully.

A standalone Android probe calling the exact new `pipa_keep::next_slot`
function passed on the device: with `vendor.boot-qti` deliberately stopped,
the function requested lazy startup, read slot B and left the service running.
This probe invoked no installer and wrote no device partitions.

Read-only preflight of the actual post-kernel boot exposed another conflict:
AnyKernel normalized six symlink modes from `0120755` to `0120777`, retaining
their targets and uid/gid. Managed-overlay comparisons now tolerate only these
symlink permission bits; target bytes, file type, ownership and regular-file
modes remain checked. This rule applies only to boot ramdisk validation, not
Android data or Wi-Fi configuration. Retention still stages privately in RAM,
does not publish data backups and never writes cust.

The captured post-kernel boot (SHA-256
`6e71341929979503d54195aa05b73671ed22397efe9fd786410dee0789964bad`)
passed offline repair with the new seed. Its kernel, first-stage init and boot
header/arguments were preserved, the seed remained embedded, and the generated
AVB digest verified. The device's boot hash remained unchanged throughout this
investigation.

After installing the new build and restarting recovery, the user installed
`RE404.zip` again. The kernel script returned zero; automatic retention selected
slot B, completed both preflight and repair with exit zero, and logged
`this recovery written back to [B]: Ok(())`. Cust payload verification passed
for 8,676,912 bytes. The captured repaired boot contains the matching seed and
managed overlay with correct permissions; its kernel and first-stage init
contents/ownership match the reference from the same kernel package.
The AnyKernel script still left an APEX/system_b cleanup-busy warning; successful
retention does not resolve that separate cleanup issue. Recovery re-entry,
Wi-Fi, data decryption and mount switching after this test still need checking.

Host tests passed for the install backend, independent UI capability, packer
and boot patcher. The two-stage build also runs the generated-artifact checks
and emits `retention-verification.json` alongside `verification.json`.

A read-only device audit ran `--retain-check _b` against the previously
installed recovery. Boot compatibility passed; the cust manifest comparison
correctly rejected the different build. The boot partition SHA-256 before and
after the audit was identical. This audit did not write either partition and
does not count as a successful hardware retention test.

The next hardware test requires installing the matching new outer installer,
then enabling retention for a compatible package that changes boot. Confirm
the retained route, new kernel and normal system boot afterward. An actual
ROM-flash retention test remains pending.

## Historical Investigation

The following investigation explains the constraints. Its broader recovery
reconstruction proposal is intentionally outside the implemented scope above.

## Existing behavior

- `crates/skyward/src/ui.rs` exposes retention only when recovery_a and
  recovery_b both exist. The same capability flag also gates dual boot;
  enabling it globally for pipa would incorrectly enable other assumptions.
- `crates/skyward-install/src/keep.rs` saves complete recovery partition images.
  After installation it writes the saved image into selected recovery slots.
- `crates/skyward-install/src/run.rs` captures before detaching partitions.
  Payload OTA restores after update_engine reports success, targeting the
  other slot. Script ZIP handling examines the next boot slot after the script.
  The explicit cust self-update path skips this retention flow.

Pipa needs a separate boot-patching backend. Writing its saved whole boot back
would undo a ROM or kernel package's new kernel and other boot changes.

## Observed official image

The supplied OS2.0.20.0.UMZCNXM_14.0 boot.img is a 134217728-byte file with a
version 3 header, 52826128-byte kernel and 20162705-byte compressed ramdisk.
Its ramdisk contains system/bin/init, system/etc/init/hw/init.rc, recovery
libraries and this service entry:

    service recovery /system/bin/recovery

Thus this sample has an entry that can be redirected. This is evidence for this
sample only, not every ROM's boot layout. The existing patcher rejects its file
size before assessing the remaining conflicts: it requires exactly 192 MiB.
Image-file size and actual partition capacity must be treated separately.

The existing overlay is built against a TWRP recovery environment. Redirecting
the official service alone would not establish compatible graphics, Binder,
USB, security services or the complete runtime. A missing dependency must not
be treated as a successful installation.

## Intended behavior

1. Keep an independent UI capability for boot/cust recovery retention. Preserve
   the existing recovery-partition and dual-boot detection.
2. Before flashing, copy the required patch program, routing data and recovery
   resources into private RAM staging. Do not depend on data, the installed
   system, or a cust mount remaining available during installation.
3. Record slot identities, both boot digests and installed routing identity.
   Confirm pipa and a usable cust recovery payload. Hashes here detect changes;
   they are not a kernel or first-stage-init allowlist.
4. Let the ROM or kernel installer finish. Read the resulting boot images
   afresh and obtain the resulting active slot. Never assume the next slot is
   always the opposite slot for script ZIPs.
5. Patch the resulting boot, retaining its kernel, first-stage init and normal
   boot arguments. Merge only managed recovery properties. Use a versioned,
   explicitly supported transformation for its recovery userspace.
6. Construct all required outputs in RAM first. Verify kernel/init preservation,
   image bounds, routing, resource completeness and patch conflicts before any
   boot write. Back up the post-ROM boot that is about to be changed.
7. Write each selected boot, fsync and verify readback. Report recovery retention
   separately from the ROM installation result. A retention failure must remain
   visible and must not trigger an automatic reboot.

## Slot policy

The next boot slot needs a valid route, even if its boot digest was unchanged.
Any other slot whose boot was changed by the package must also be assessed.
Unchanged non-target slots are left alone. Already valid routes are idempotent.
The cust self-installer continues to use its own installation path.

Do not blindly run retention over a partially written image after an installer
failure. Inspect it and report an inability to retain when structural validity
or the intended next slot cannot be established.

## Cust policy

If the verified cust payload survived the installation, only restore the boot
route. Do not reinstall cust or create a 1 GiB backup for every kernel ZIP.

If the package overwrote or formatted cust, boot routing alone cannot recover
Skyward. Recreating the dedicated directory requires a supported, validated
filesystem and staged payload; fail explicitly when those prerequisites are
absent. Never format cust automatically or overwrite unrelated contents.

## Implementation boundaries

- Add a distinct retention capability, rather than setting
  `recovery_partitions=true` for pipa.
- Generalize the installer's retention backend: existing recovery-image restore
  and new pipa boot-route patching share events, not raw write behavior.
- Refactor the patcher's transformations and validation into a reusable library
  or stage its static executable plus generated overlay during image packing.
  Embed these resources in both delivery methods from the same source build.
- Use block-device capacity to size the output. Accept bounded boot files with
  trailing padding independently from the partition's total size.
- Retain current overlay ownership/conflict checks. For an official or custom
  base with different managed files, add a validated compatibility path rather
  than disabling checks and overwriting unknown files.
- Separate successful ROM installation from failed recovery retention in UI
  state and logs.

## Validation sequence

First use regular image files only: currently patched boot, a kernel-modified
boot, the supplied official boot, and unsupported/truncated inputs. Check
idempotence, both slot choices, property merges, unknown conflicts and exact
kernel/init preservation.

Then temporarily boot a patched result with its own new kernel retained.
Confirm Android normal boot and recovery boot routing independently. Exercise
display, touch, Wi-Fi, decryption, MTP and partition release. No device partition
write is justified by a passing host patch test alone.

Finally exercise a controlled real install and retention. Full OTA, kernel ZIP,
script failure and cust replacement are separate cases. The currently passing
vendor-runtime v8 tests do not cover these scenarios.

## Concrete implementation plan

Follow-up comparison of the v8 generated recovery ramdisk and the supplied
official boot found 1059 versus 842 entries: 235 shared entries were identical,
198 shared entries differed, and 626 entries existed only in v8. In particular
system/bin/init and first_stage_ramdisk contents differed. These are archive
entry comparisons, not a claim that every different file is needed.

Implement two explicitly different patch modes:

1. Managed overlay repair. For a resulting boot whose recovery environment is
   compatible with the existing installer, reuse the overlay patcher. Validate
   its manifest, merge properties and restore routing. This handles suitable
   kernel packages without replacing their new kernel.
2. Recovery environment reconstruction. For a ROM that replaced the recovery
   environment, use a validated pipa ramdisk template and an explicit rule set
   describing what is retained from the new boot. A stock service entry alone
   is insufficient evidence to select this mode.

The first backend object can be `PipaKept`, alongside the existing `Kept`.
Before flashing it owns an immutable copy of the active boot ramdisk,
both boot identities and the verified cust payload in RAM. A boot loaded only
temporarily must not silently use an unrelated installed boot as its template.
The capability is enabled only when these resources can be captured and
validated. Repeated installs capture fresh state, rather than reusing a stale
snapshot from an earlier package.

`wanted(next)` selects the next slot plus any other slot whose boot changed.
`prepare(slot)` reads the post-install boot and creates an output without
writing a partition. `write(prepared)` runs only after all selected outputs
passed validation. Both reuse the existing progress and retention-result events.

For reconstruction, start the experiment offline with the official image.
The new boot header settings, kernel, normal arguments, root init entry,
system/bin/init and first_stage_ramdisk tree must have explicit provenance.
Retaining these new first-stage components is a goal to validate, not proof
that they understand the older recovery configuration. Recovery executables,
their compatible libraries, device setup and the current minimal runtime
must be treated as a coherent environment. Preserve root modifications and
unknown content unless an explicit compatibility rule accounts for them;
reject unresolved conflicts. Do not merge different libc/linker stacks by
copying only files that happen to be missing.

The initial offline command should take:

    prepare-retention POST_INSTALL_BOOT VALIDATED_TEMPLATE OUTPUT_BOOT REPORT

It must produce a machine-readable provenance report and never open device
block nodes. Report kernel/init hashes, header/argument changes, file origins,
unsupported conflicts, packed size and target capacity. Add regular-file tests
for a changed kernel, official recovery layout, idempotence, modified root
files, too-small capacity and corruption before wiring the writer into UI.

Only after the reconstructed official sample passes temporary recovery boot
and controlled normal-boot validation should reconstruction be exposed as
automatic retention. Until then support managed repair explicitly, and show
unsupported-layout failures rather than presenting broad ROM support.
