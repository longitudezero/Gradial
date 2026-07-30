# Milestone 2 custom icon results

## Scope

This bounded slice adds per-direction custom icons without broadening the
gesture or action boundaries:

- PNG and SVG selection in Global and application-profile menus;
- strict 256 KiB and 512×512 source limits;
- copying into Gradial-owned managed storage;
- load-time decoding and texture registration;
- the same icon rendering in production overlays and Settings previews;
- a built-in placeholder for missing or damaged managed assets.

Portable export-side icon copying and managed-asset garbage collection remain
separate follow-up work.

## Schema-five model and migration

Schema five adds one optional icon reference to each menu slice:

```json
{
  "label": "North",
  "action_id": null,
  "icon": {
    "asset_id": "a5fda722-80ba-46ee-8983-6ff7bd0c6a66",
    "format": "png"
  }
}
```

The model contains no source or managed filesystem path. `asset_id` must be a
non-nil UUID and `format` is the closed `png`/`svg` enum. Gradial derives the
only permitted managed filename as `<asset-id>.<format>` inside its own
`%LOCALAPPDATA%\Gradial\icons` directory. Unknown metadata and formats fail
normal strict deserialization.

Schema-one through schema-four files gain no icon assignments in memory and
validate as complete schema five. Their source bytes remain untouched until an
explicit successful save. An icon field inside any older schema is rejected as
ambiguous rather than interpreted as newer data.

The compiled profile resolver freezes the icon-reference list alongside slice
count, labels, and action references. Changing configuration after
activation-down cannot switch an icon for an open gesture.

## Import and managed storage

The native picker filters to PNG and SVG. The importer then independently
checks the selected source:

- it is an absolute regular file;
- its extension is PNG or SVG, case-insensitively;
- the source is non-empty and no larger than 256 KiB;
- decoded logical width and height are each from 1 through 512;
- PNG structure and pixels decode successfully;
- SVG structure parses successfully.

SVG scripts, document declarations/entities, foreign objects, embedded raster
images, and text nodes are rejected. Text should be converted to vector paths
before import. This keeps SVG sources self-contained and prevents them from
loading external or unexpectedly large resources.

Only after validation does Gradial generate the asset UUID, write a new
temporary file in the managed icon directory, flush it, and rename it to its
final managed name. The draft receives the reference only after that completes.
Discarding or removing an assignment deliberately leaves the managed file
alone because another profile, the saved configuration, or a backup can still
reference it.

## Load-time renderer cache

Before input can start, the icon manager enumerates every unique reference in
the complete configuration. PNG pixels are decoded and SVG vectors are
rasterized, then registered as egui textures before the prewarmed overlays
become ready. All monitor renderers and the Settings renderer share this one
cache.

An invalid, missing, or unreadable managed source records a fixed fallback
category and caches a placeholder state. It never prevents configuration,
profiles, input, or actions from loading, and no decoder runs on activation.
Choosing a new icon or importing a draft extends the cache while the running
menu continues to use its saved frozen references.

Icon directions place a fitted 30-DIP image inside the ring and move a smaller
label outward with a bounded contrast backing. Non-square images preserve
their aspect ratio. Directions without an icon retain the exact original label
layout. A missing icon draws a theme-aware question-mark tile in the same
bounded image rectangle.

Appearance size presets scale the image and label at paint time. Gesture
thresholds, angular boundaries, overlay dimensions, profile resolution, and
geometry caches remain unchanged.

## Settings behavior

The selected direction in either **Menu** or **Profiles** exposes:

- **Choose icon…** when unassigned;
- the assigned PNG/SVG type;
- **Replace icon…**;
- **Remove icon**;
- local success or field-specific failure feedback.

The draft, discard, guarded-close, import, export, and save rules apply to icon
references exactly as they do to labels and actions. Four/eight conversion
preserves cardinal icon assignments and stashes diagonal assignments.
New/duplicated profiles copy their complete menus and therefore reuse the same
managed asset references rather than duplicating source files.

Current JSON export writes schema five and preserves icon references, but it
does not yet create the optional adjacent `icons` directory. The Settings UI
and documentation state that icon files must currently be copied separately
for portability.

## Automated coverage

The complete debug suite contains 202 tests:

- 150 `gradial-app` tests;
- 52 `gradial-core` tests.

Coverage added by this slice verifies:

- closed icon formats, derived managed filenames, and nil-ID rejection;
- schema-one through schema-four migration and older-schema smuggling
  rejection;
- icon references frozen in resolved menus;
- case-insensitive PNG/SVG type selection;
- bounded PNG and SVG decoding;
- disallowed SVG content and oversized logical dimensions;
- atomic managed copying and texture registration;
- load-time cache reconstruction;
- missing-file placeholder selection;
- aspect-preserving renderer layout and outward labels;
- draft-only assignment, field-specific failures, and discard.

Formatting, strict Clippy, complete unit tests, and debug build checks pass.

## Native Windows evidence

`smoke-settings-editor.ps1 -CaptureScreenshot` passes in a disposable
`LOCALAPPDATA` tree. The extended harness:

1. creates a transparent 48×48 PNG outside the managed icon directory;
2. requests a real icon import for Global north;
3. requires exact validation/caching metadata;
4. verifies exactly one UUID-named managed PNG;
5. captures the real icon editor and production renderer preview;
6. copies the complete menu into an application profile while retaining the
   same asset reference;
7. exports schema five at the unchanged revision and verifies the reference;
8. confirms active schema-two bytes remain untouched before save;
9. saves revision 2 and verifies both assignments plus the one shared source;
10. invokes the reloaded production menu through the saved Ctrl+M5 binding.

Selected source paths, imported labels, action payloads, and captured profile
identity remain absent from Gradial-authored logs. Cleanup removes only the
exact disposable tree and process created by the harness.

## Remaining boundary

The optional adjacent `icons` directory for portable export/import remains the
next bounded configuration-transfer slice. Safe reclamation of managed assets
requires reference analysis across the active configuration and backup, so it
is intentionally deferred rather than deleting files during draft edits.
