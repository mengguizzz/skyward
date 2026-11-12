# Xiaomi Pad 6 tablet UI

The UI keeps the existing page renderer and recovery operations. On displays
whose shortest side is at least 600 dp, a tablet shell adds a rotation control.
Phones keep their existing viewport and touch handling.

## Layout and rotation

- Portrait uses a single page.
- Landscape renders the actual portrait Home page on the left, including its
  search field, device card, coloured icons and grouped entries. There is no
  separately maintained menu. Home is rendered from the same live state.
  Tapping an entry opens its page on the right; all subsequent nested pages
  and Back actions stay on the right and retain the ordinary navigation stack.
- A single status bar spans the whole display above both
  columns. Detail pages do not draw their own status bar.
- The Home pane and detail page scroll independently.
- A 20 dp icon to the left of the right-hand status readings toggles between the original
  portrait (rotation 0) and the user's confirmed landscape (rotation 3).
  Its touch target remains 44 x 44 dp. Its position follows the actual left edge
  of the readings, including changes in battery, network and temperature.
  There is no text label or additional app toolbar above the panes.
- Legacy rotation 1 becomes 3, and rotation 2 becomes 0.
- The `rotation` setting is saved through the existing settings keeper.
- Rotation is manual. Automatic rotation using the accelerometer is not
  implemented; this does not start the vendor sensor service.
- Framebuffer dimensions, display mode, refresh rate, and kernel are unchanged.
  The composed canvas is rotated in software; touches use the inverse mapping.
- Pages keep their state and navigation stack across rotation. In-progress
  gestures and confirmation sliders are cancelled, and size-dependent drawing
  caches are rebuilt. A running terminal receives its updated rows and columns.
- Before reaching Home, startup unlock/rescue screens have no sidebar.
- Sidebar navigation uses ordinary Back and page-entry handlers. Existing
  restrictions on leaving active writes and unsaved editors remain in effect.

## Source

On pipa's first interactive menu start without a saved explicit language,
the existing language page is shown before unlock/Home. Its bilingual heading
and native language names offer Simplified Chinese, Traditional Chinese and
English. A selection continues to the original start page and uses the normal
settings keeper. Existing choices skip setup, and automated recovery requests
and fastbootd are not intercepted. During setup the Home sidebar is hidden.

- `crates/skyward-ui/src/tablet.rs`: shell layout, rendering, transformed touches,
  independent navigation scrolling, and guarded navigation.
- `crates/skyward-ui/src/app.rs`: shell state, page composition entry points,
  tablet metrics, and terminal resize effect.
- `crates/skyward-ui/src/settings.rs`: backward-compatible rotation persistence.
- `crates/skyward-ui/src/term.rs`, `crates/skyward/src/pty.rs`, and
  `crates/skyward/src/ui.rs`: terminal and pseudo-terminal resize.
- `crates/skyward-ui/src/icons.rs`: rotation icon in the existing icon system.
- `crates/skyward-ui/src/theme.rs` and `widgets.rs`: tablet spacing, compact
  headings, and fixed-height light/dark segmented selection.

## Host verification

```sh
cargo test --locked -p skyward-ui
cargo run --locked -p skyward-preview -- OUTPUT_DIR --tablet
```

The preview uses pipa's actual 1800 x 2880 panel and 400 dpi configuration.
It renders both supported orientations in both themes. Landscape previews navigate
through the real Home device card using physical panel coordinates and assert
that the About page opens on the right.
PNG files are exported upright for inspection.

Before calling a release hardware-tested, check rotation in both landscape
directions, sidebar and page scrolling, password keyboard, swipe confirmations,
screen off/on, terminal resizing, and settings persistence on the tablet.

## First-language device verification

On 2026-11-12, the language setup was verified on pipa after clearing only the
explicit `language` setting to simulate a first interactive start. The picker
appeared and the user selected English. The device then saved both `language`
and `spoken` as `en`, while retaining rotation 3 and the other settings.
The metadata filesystem was read-only again after the selection was saved.
This verifies the setup and save path; persistence across another recovery
restart was not separately retested in this check.
