# NURBS raster performance and release build

Regions share immutable curve spatial indices between independent row bands.
The pixel buffer is partitioned into disjoint slices, so workers never write the
same pixel. Only after all bands complete is the PNG and opacity-mask art
published. The caller's cancellation token is explicitly captured by workers;
thread-local progress callbacks stay on the caller. Constant profiles bypass
nearest-distance queries while preserving outer clipping and the full inner fill.

Colored NURBS strokes shade disjoint row bands. Each band processes every
intersecting section in the original parameter order, including source-over
compositing and the final object opacity. Coverage spreading runs after joining
the bands, preserving the two-pixel halo at raster-tile boundaries. Color
conversion happens before parallel work; workers receive numeric samples only.
The shared gradient-mesh rasterizer benefits from the same band execution.

The native jobs use the existing Rayon dependency and its reusable pool. Small
jobs remain serial (at least 32K estimated covered pixels per worker). Worker
requests come from the existing render-thread preference, including 0 for serial,
and the `VECTORCRAFT_RENDER_THREADS` environment override; automatic mode leaves
a core free and requests at most eight workers. WASM uses the serial path.

A 32 MiB frame cache retains colored-strip geometry by owning-node identity,
stroke, pixel scale and color-settings generation. Panning reuses the geometry;
node/style changes, zoom and color-settings changes invalidate it. The cache keeps
its source node alive, preventing address reuse from producing false hits.

## Region defaults

New regions and the panel's Default button use `[[0,1],[1,0]]`. The distance ratio
is zero at the inner curve and one at the outer curve, so this gives a continuous
fade from full alpha to zero. Existing saved profiles retain their chosen points.

## Debris display

Each debris has `display: normal | mute | outline`. The selected-object panel and
the base curve's Attached debris list both expose the mode. Mute hides the derived
art. Outline displays thin black paths and warped image frames instead of fills
and textures. Sources, bindings, phases and normal generated geometry are retained;
referencing another debris uses its normal geometry in every display mode.
Modes participate in Undo and persist in native files; older files default to normal.

## Production build

`cargo build --release --frozen --offline -p vectorcraft`

The workspace release profile uses opt-level 3, fat LTO, one codegen unit, no debug
information and no incremental compilation. Panic unwinding remains available.
The executable is `target/release/vectorcraft.exe`. No CPU-specific instruction
set is forced; the renderer retains its runtime SIMD selection.

The compiler job limit affects build memory/compilation only; it does not restrict
runtime raster workers.

## Reproducible benchmark

`cargo run --release --frozen --offline -p vectorcraft-testkit --example nurbs-perf`

The benchmark measures region refreshes with an approximately 1024px distance
mask (PNG encoding included), and a 1600px translucent self-crossing colored
stroke with a warm geometry cache. It compares serial execution with up to eight
workers, checks byte-identical mask PNGs and rendered RGBA, and prints average
milliseconds and speedups. It compares thread counts within the same optimized
binary, rather than comparing a debug build with a release build.

## Validation

The focused NURBS run passed all 71 tests (17 document, 28 engine, 16 renderer,
10 UI). These include serial/parallel byte equality, cancellation, tile boundaries,
cache reuse and invalidation, mode Undo/Redo, legacy deserialization, chained
references, and raster outline rendering. All six existing live-rasterizer tests
also passed, including mesh slivers, smooth colors and edge-to-edge coverage.
Rustfmt and the tracked whitespace check passed.

The full desktop Release build completed successfully with the production profile
above. A fresh Clippy run for document/renderer code completed without errors;
style warnings remain in geometry, hit testing and a renderer doc comment.

The optimized executable passed 20 native checks: the Display combo and attached
rows, independent vector/image modes, unchanged source bindings, one-step Undo and
Redo, native save/reopen, SVG/PDF export, and the Region Default button after Linear.
Evidence is in `target/nurbs-optimized-native/state.json` and the adjacent screenshots.

## Measured production timings

2026-10-27, eight workers, average of five operations per mode:

| Operation | Serial | Eight workers | Speedup |
| --- | ---: | ---: | ---: |
| Region refresh, approximately 1024px mask, PNG included | 262.97 ms | 65.27 ms | 4.03x |
| Colored stroke, 1600px, warm geometry cache | 356.06 ms | 162.59 ms | 2.19x |

Both region PNGs and the complete rendered RGBA were byte-identical. These are
scene-specific measurements, not a guarantee for every document or viewport.

For this measurement the example was linked with the exact four production
rlibs reported by `cargo build --release --frozen --offline -p vectorcraft
--message-format=json`, using rustc opt-level 3, fat LTO and one codegen unit.
This avoids rebuilding dependencies with the testkit's different feature set.
The local reproducer is `target/nurbs-bench-production.ps1`; outputs are in
`target/nurbs-release-timings.log` and `target/nurbs-release-artifacts.jsonl`.
The Cargo example command above remains the portable source-level entry point.