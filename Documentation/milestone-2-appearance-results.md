# Milestone 2 appearance configuration results

## Scope

This bounded slice completes the v1 appearance controls while deferring custom
icons:

- one RGB accent color;
- explicit light or dark labels;
- compact, standard, or large visual sizing;
- draft-safe Settings editing;
- the production renderer in every live preview;
- atomic persistence and immediate post-save application.

The original blue accent, light labels, and standard size remain the default.
Appearance is global: it applies consistently to the Global menu, every
application profile, onboarding practice, and their Settings previews.

## Schema-four model and migration

Schema four adds:

```json
{
  "appearance": {
    "accent_color": {
      "red": 124,
      "green": 154,
      "blue": 255
    },
    "label_theme": "light",
    "size_preset": "standard"
  }
}
```

RGB components are bounded naturally as unsigned bytes. Label theme and size
preset are closed enums, so arbitrary themes, custom scales, skins, and fonts
cannot enter the model.

Schema-one files gain action, activation, and appearance defaults in memory.
Schema-two gains activation and appearance; schema-three gains only appearance.
All three migration paths validate as complete schema four without rewriting
their source bytes. An older file containing an appearance field is rejected
as ambiguous rather than silently interpreting data from a newer schema.

Import uses the same migration path. Export always writes a complete
schema-four draft. A later explicit save preserves the old primary as its
original schema in `config.json.bak`.

## Renderer behavior

The configured accent replaces the foundation outline, divider, and selected
slice colors. The default accent reproduces the existing blue palette. Light
labels retain the original dark ring and center. Dark labels choose a
contrasting light ring and center plus a small bounded light backing for each
label, keeping long labels readable when they extend beyond the ring over an
arbitrary foreground application.

Size presets multiply visual coordinates and fonts by:

| Preset | Visual scale |
| --- | ---: |
| Compact | 0.85 |
| Standard | 1.00 |
| Large | 1.15 |

The multiplier is applied at paint time to the existing cached four/eight
geometry. Gesture center/entry radii, angle boundaries, hysteresis, raw-delta
edge assist, and overlay-window dimensions do not change. Thus appearance
cannot alter which direction a physical gesture selects.

Appearance is copied into each prepared render frame. Saving updates the
existing prewarmed pool and restarts interaction for the new configuration
revision; it does not rebuild geometry caches or create windows.

## Settings behavior

The **Appearance** tab includes:

- an RGB color picker with hexadecimal display;
- **Light labels** and **Dark labels**;
- **Compact**, **Standard**, and **Large**;
- **Restore original appearance**;
- a live Global-menu preview.

The Menu and Profiles previews receive the same proposed appearance. Changes
remain in the private full-configuration draft, participate in dirty-state and
guarded-close behavior, and can be imported or exported before save. Discard
restores the saved appearance. Only a successful complete save updates
production.

## Automated coverage

The debug verification suite contains 192 tests:

- 142 `gradial-app` tests;
- 50 `gradial-core` tests.

Coverage added by this slice verifies:

- original appearance defaults and serialization;
- closed label/size enums and exact preset scales;
- schema-one, schema-two, and schema-three in-memory migration;
- rejection of appearance fields smuggled into older schemas;
- atomic schema-four initialization, import, export, save, and backup behavior;
- preservation of the default foundation palette;
- the contrasting dark-label palette and backing;
- draft-only appearance edits and discard;
- size-aware preview hit normalization;
- post-save pool appearance replacement without geometry-cache invalidation.

The complete script syntax, formatting, strict Clippy, unit-test, and debug
build check passes.

## Native Windows evidence

`smoke-settings-editor.ps1 -CaptureScreenshot` passes against a disposable
`LOCALAPPDATA` tree. In addition to its existing editor coverage, the harness:

1. imports a schema-two fixture and receives the default appearance only in the
   private draft;
2. changes the draft to RGB `(224, 96, 72)`, dark labels, and large size;
3. captures the real Settings preview and its long-label contrast backings;
4. exports schema four at the unchanged revision and verifies every appearance
   field;
5. confirms active schema-two bytes are still untouched;
6. saves revision 2 and verifies the exact schema-four appearance on disk;
7. requires the production pool's `renderer_appearance_updated` record;
8. invokes the reloaded menu through the saved Ctrl+M5 production binding.

The old schema-two primary remains intact in the backup. The fixture never
reads or writes the user's Gradial data and removes its exact temporary tree.

## Remaining boundary

V1 color, label contrast, and size configuration are complete. Per-slice custom
PNG/SVG icon metadata, bounded decoding, GPU upload at configuration load,
placeholder fallback, editor assignment, and optional export-side icon copying
remain the next separate slice.

Follow-up: per-slice import, validation, managed storage, caching, fallback,
editor assignment, and rendering are now complete in
[`milestone-2-custom-icons-results.md`](milestone-2-custom-icons-results.md).
Portable export-side icon copying remains deferred.
