# NURBS and pasted-curve panel

Open **Window > NURBS**. The panel participates in the existing dock/floating-panel
system and its icon rail. Stage two adds UI; the same complete-model API remains usable.

## Creating and naming

1. Draw and finish the NURBS curves. Name them in Layers as needed.
2. Make an ordinary group. The first NURBS in stored bottom-to-top order is the base.
3. Select the group and click **Paste NURBS from Group** in the NURBS panel.

Other object kinds remain in the group. Each pasted record inherits its source node's
name. Duplicate sibling names gain ` (2)`, ` (3)`, etc.; unnamed objects use `curve-ID`.
Names are bounded to 256 UTF-8 bytes. Old pasted records keep their saved names: consumed
source-node labels cannot be reconstructed retroactively.

## Selection and movement

The hierarchy lists the base and nested pasted curves. Selecting the base enables the
ordinary NURBS control editor. Selecting a pasted row switches that tool into pasted
selection mode; the selected contour is orange and the underlying parent is dashed.

Click another pasted contour to select it. Drag it along its parent's curve to translate
its interval. Mouse release commits one Undo step; Escape cancels the drag. Delete removes
the selected paste and restores its parent's interval. Selection is transient and bound
to the current document. Choosing the base row returns to control-vertex editing.

## Range strip and numeric settings

The strip exposes `0..1` for an open parent and `0..2` for a periodic parent. The first
turn is blue and the second orange, with the seam marked at `1`. A crossing interval such
as `[0.9,1.1]` can be moved continuously into `[1.1,1.3]` without jumping to the first turn.
Drag either white handle to change an endpoint, or drag between handles to translate the
interval. Shift-drag also translates a narrow interval. A strip drag is one Undo step;
Escape cancels it. Active intervals still cannot overlap, and a periodic interval must
remain shorter than a full turn. Invalid previews leave the last valid state intact.

Start/End may be shown in parameter or parent-local length units. Scale may be negative
or zero. Numeric fields are drafts: **Apply** commits them together as one Undo step;
**Reset** reloads the actual geometry. The selected contour is editable in its currently
posed parent frame, including nested length-bound curves after ancestor deformation.

## Duplicate and temporarily disable

Every pasted row has an activity checkbox; the selected paste also has **Enabled**.
Turning it off restores the parent interval while preserving the full source, name,
range, scale and nested records. Disabled records do not reserve an active interval.
Old files without this flag remain enabled by default.

**Duplicate** makes an independent copy, adds a unique name suffix and selects it.
The copy starts **disabled at the same interval** because active copies may not overlap.
Move it with the strip, fields or canvas, then enable it in a free interval. A selected
disabled copy can be picked and dragged even when it coincides with the original.
Duplication, enablement, removal and all parameter edits support native save/reopen
and Undo/Redo.

## API additions

- `nurbs.paste.update {id,name,enabled?: bool,u_range?,l_range?,scale?}` can toggle activity.
- `nurbs.paste.duplicate {id,name,newName?}` returns `{id,name,enabled:false}`.
- `nurbs.get` includes `enabled` in each direct pasted summary.
- The complete model stores optional `enabled`; missing means true. `nurbs.create` and
  `nurbs.update` continue to accept the entire hierarchy. UI edits of nested records use
  the complete-model route.

See [Pasted NURBS API](nurbs-pasted-api.md).

## Verified 2026-10-25

- All 97 geometry tests passed in direct Rust/LLVM verification.
- Targeted Cargo suites passed 43 tests: 12 engine, 25 geometry, 2 tools and 4 UI.
  UI tests include live nested length binding, hierarchy selection, periodic range-strip
  movement across the seam with one Undo and Escape, and C priority/repeat/non-Latin input.
- Native desktop build succeeded. Live UI checks exercised the group-conversion button,
  inherited Curve_1 name, Duplicate -> Curve_1 (2) disabled, selecting/moving the coincident
  disabled copy on the canvas, its row checkbox, and enabling it in a free interval.
- A native strip drag moved the original from [0.9,1.1] to approximately [1.0818,1.2818]
  while displaying the second turn continuously; one Undo restored [0.9,1.1].
- The original was disabled through its checkbox; native save/reopen retained both names,
  ranges and activity flags [false,true]. The isolated test application was closed.

Evidence: target/nurbs-panel-tests-final.log (engine/geom/tools; its first UI regression
failure was corrected), target/nurbs-panel-ui-tests.log (all four UI checks pass),
target/nurbs-panel-build.log, target/nurbs-panel-native-report.json and the
nurbs-panel-*.png native screenshots. Full workspace CI has not been run.

## 2026-10-26: separate creation/editing (source, native verification pending)

- **Create NURBS Tool** (`nurbsCreate`): clicks create a new curve regardless of existing selection. Enter/Escape finishes and selects **Edit NURBS Tool**; C closes the newly drawn curve periodically and selects editing. Already created vertices remain on finish.
- **Edit NURBS Tool** (`nurbs`): click a curve to select it; drag its control vertices. Empty clicks cannot create or append a curve. Existing weight, corner, refinement and Alt-double-click color gestures belong here. Pasted hierarchy selection uses this editor.
- Two separate toolbar buttons, using the existing curvature and direct-selection icons. The old `tool.select {tool:"nurbs"}` now means editing only; use `nurbsCreate` for creation. Geometry create/update APIs are unchanged.
- Colored control bands widened to three pixels around the seven-pixel selection marker, with light/dark halos. Color opacity does not hide editing markers.
- The small page-background swatch moved to the bottom left, immediately before Zoom. It changes the active artboard's view-only paper color through the existing color picker. Stroke caps/joins remain under **Stroke settings** in the NURBS panel.

Validation evidence for the preceding N7.3/N7.4 revision: `isolated-build_20261026_0331_fd065d82` check/test/build all exit 0, 59 filtered NURBS tests, 12 native UI checks; network restoration completed and no blocked events recorded. N7.5 formatting and Windows PowerShell syntax/plan checks pass; its build and native UI are not run yet.

## 2026-10-26: N7.5 verification follow-up

The first isolated N7.5 run (`isolated-build_20261026_0403_7c412cfc/run_20261026_020347`)
passed `check` but stopped during engine tests: the existing interactive creation test
still selected `nurbs`, which now edits only. Build and native UI stages did not run.
Restoration completed according to the isolation state and the user's STEP 3 output.

The regression now selects `nurbsCreate` for drawing and explicitly checks that Enter
switches to `nurbs` before testing drag/cancel, weights and history. The build runner
also prints the tails of both stdout and stderr on failure, so test assertion details
appear in the console rather than only in the saved stdout log.

Rust formatting/syntax, PowerShell parsing and the build runner's Windows PowerShell 5.1
`-Plan` mode passed. The corrected tests, desktop build and native UI still require a
new isolated Check-All run; no new runtime pass is claimed.

## 2026-10-26: linked NURBS region fills

The NURBS panel can construct a fill object from two selected periodic curves.
It has outer/inner source selectors, role swapping, source editing and a live
alpha-profile graph. Broken links render empty and have a warning in Layers.
See [NURBS regions](nurbs-region.md) for the exact distance rule, commands,
source-paint behavior and the raster-mask resolution bound.
