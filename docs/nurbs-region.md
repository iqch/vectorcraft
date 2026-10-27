# NURBS regions

Status: 2026-10-26 — built and verified: 45 filtered NURBS tests and 20 native checks passed.

Select two periodic native NURBS curves and choose **Create NURBS Region** in
Window > NURBS (also Object > NURBS > Create NURBS Region). The curve with the
larger bounding-box area is initially the outer source. The panel can swap roles
or replace either source, including leaving it unassigned. Source curves remain
separate editable objects; construction clears their own paints so they do not
show through the resulting fill. This is part of the single creation Undo step.
The creation API can retain source paints with `clearSourcePaint:false`. Assigning a new editable periodic source clears its paints as well.

The result has standard object fills and transparency; its strokes are ignored.
All geometry is controlled by the source curves in document coordinates: edit or
move the sources to reshape/move the region. **Edit** beside a source selects it
with Edit NURBS; **Select linked NURBS region** returns to the result.

For point p:

- outside the outer contour: alpha multiplier 0;
- inside both outer and inner contours: alpha multiplier 1;
- elsewhere inside outer: `profile(d_inner / (d_inner + d_outer))`.

Distances are Euclidean distances to the curves, not to their control polygons.
The profile is the same editable, shape-preserving cubic interpolant as the
colored-stroke profiles. It starts with `[[0,1],[1,0]]`: alpha fades continuously
from full at the inner curve to zero at the outer curve. **Default** restores it;
**Linear** selects `[[0,0],[1,1]]`, and **Invert** reverses its values.
Inner/outer contour
intersection or incomplete nesting is allowed; the outer contour always clips
all output. Fill rule is nonzero winding.

The graph previews live. A drag is one Undo step; Escape cancels it. Double-click
the graph to add a point, right-click an interior point to remove it. Linear
selects a linear ramp; endpoint y values are editable.

Deleting a source, switching it to non-periodic topology, assigning the same
source to both roles, or removing either assignment yields empty derived art.
The Layers name has a `⚠` prefix, and the panel explains the broken link. Undo or
replacing/fixing the source restores the region.

The node stores its source IDs and profile in `nurbs_region`. Its ordinary path
and standard opacity mask are derived atomically after engine edits, including
source tool previews. Native files retain links, the spline and mask pixels.
Standard mask support carries the appearance through SVG/PDF export.

The alpha field is rasterized at the Document Raster Effects resolution, bounded
to 1024 pixels on its longest side. This bounds memory during editing; it is a
raster approximation at high zoom. Distances use an adaptive flattened NURBS
outline (initial display approximation 0.025 document units) and a bounded BVH of
line segments, with a flattening tolerance of 5% of a mask pixel. Inner full fill
is also represented as a vector shape in the mask, and the outer domain is an
exact display path clip. Unchanged geometry and profiles reuse mask pixels.

## Commands

| Command | Parameters |
|---|---|
| `nurbs.region.create` | `outer`, `inner` (or two selected sources); optional `profile:{points:[[x,y],...]}`, `clearSourcePaint` (default true) |
| `nurbs.region.get` | `id` (or one selected region); returns links, profile and `problem` |
| `nurbs.region.update` | `id`; optional `outer`, `inner` (ID or null), `swap`, `profile` |

All profile coordinates are finite and in [0,1]; x is strictly ordered with
endpoints 0 and 1; there must be 2–128 points. Invalid spline edits are rejected
without changing the document. Broken source relationships are valid editable
object states that intentionally draw empty.

## Revision history

- 2026-10-26: linked region model, mask materialization, commands, graph and source
  controls, layer warning, geometry/engine/pixel/panel regression tests.

## Verification evidence (2026-10-26)

- `cargo test --frozen --offline -p vectorcraft-doc -p vectorcraft-engine -p vectorcraft-render -p vectorcraft-ui-egui --lib nurbs`: 45 passed (doc 8, engine 20, render 8, UI 9). Includes malformed spline rejection, container metadata preservation, source edits/removal/periodicity, replacement, Undo/cancel, native mask persistence, SVG/PDF output and pixel alpha/gradient/cache tests.
- `cargo build --frozen --offline -p vectorcraft`: succeeded; updated `target/debug/vectorcraft.exe`.
- Owned native session: 20 checks in `target/nurbs-region-native/state.json`. Actual graph drag changed endpoint alpha to 0.478873252868652; one Undo and Redo round-tripped the gesture. Invert, Swap and Edit source buttons worked. A source control drag changed the rendered image; its Undo restored a byte-identical PNG. Breaking periodicity emptied the region; screenshots visually confirmed the panel error and red warning in the actual Layers row. Undo repaired it. Replacing the inner curve, saving and reopening retained source IDs and spline. The test app closed.
- Screenshots: `target/nurbs-region-native-initial.png`, `target/nurbs-region-native-graph-drag.png`, `target/nurbs-region-native-source-edited.png`, `target/nurbs-region-native-broken-panel.png`, `target/nurbs-region-native-broken-layers.png`, `target/nurbs-region-native-reopened.png`.
- Touched Rust files pass rustfmt; the changed diagnostic PowerShell script parses; tracked touched code passes the whitespace diff check.
- Final non-strict Clippy completed with no diagnostics in the new region modules. Strict `-D warnings` is blocked by existing `collapsible_if` diagnostics beginning in `crates/doc/src/hit.rs:499`; the final log also records existing diagnostics elsewhere in previous NURBS/UI work. No full workspace CI or WASM run is claimed.
- Harness corrections: the headless egui frame explicitly clears texture deltas; the native launch cell stays alive while the owned process is exercised. `ui.inspect` does not contain painted Layers warning text, so that check uses the actual screenshot rather than its JSON fields.

Parallel mask generation, the revised default and optimized builds are documented
in [NURBS performance](nurbs-performance.md).
