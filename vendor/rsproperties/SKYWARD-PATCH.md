# Skyward Android property path fix

This is rsproperties 0.6.0 from crates.io, distributed under its included
Apache-2.0 LICENSE. The registry archive checksum was
`e92cdbbe82223006ba7f16e6178dd9b24fbb967d000a096e8dc4129560d7ab41`.

The only functional local change is in `src/build_property_parser.rs`:
Android property-file canonicalization opens the file and reads its resolved
path from `/proc/self/fd/<fd>`, rather than calling Bionic `realpath`.
The descriptor closes before returning. Other targets keep canonicalize.

This preserves the existing parser, property imports, override ordering,
symlink-aware recursion detection, and import budgets. The local override
in the workspace Cargo.toml makes CI use this change with `--locked`.

Reason: the pipa recovery kernel under test leaks mount references after
path-stat. Bionic realpath calls path-stat even when the application's
explicit metadata checks have already been replaced with descriptor checks.
See `tools/pipa-ci/STARTUP-MOUNTS-20261110.md` in the Skyward repository.
