# Colored NURBS stroke — implementation draft

Status: 2026-10-26 04:00. N7.3/N7.4 verified: offline check, 59 NURBS tests, build, and all 12 native UI checks pass (isolated-build_20261026_0331_fd065d82); restoration completed, no blocked events observed. N7.5 separate tools, stronger control-color bands and status-bar page-color button are prepared in source; compilation/native verification pending.

A colored NURBS object retains its source curve and ordinary fill. Its special stroke uses per-control RGBA, a width spline, a signed normal-offset spline and an opacity spline across the whole local width. Corners, periodic topology and pasted source curves retain their existing model.

## Model and semantics

- Each control accepts optional `color: [r,g,b,a]`, components 0..1. Missing colors are opaque white, including in old documents. Transforms and pasted materialization retain colors.
- `color_stroke` is an optional live-NURBS attribute. Missing/null means an ordinary NURBS stroke.
- `interpolation`: `rgb` (default) or `hsv`. RGB follows the rational de Boor basis. HSV uses rational de Boor interpolation with the shortest hue arc at each interpolation step; achromatic colors inherit the neighboring hue. Alpha is linear under the same rational weights.
- `width.points`: spline samples `[x,y]`, x in [0,1], y in [0,2]. Final width is the system stroke weight times y.
- `offset.points`: y in [-1,1]. The signed displacement is y times `offset_amount` in document units. Positive goes to the left of the direction of travel in the application's y-down coordinates. It moves the stroke, not the source or fill.
- `opacity.points`: x=0 at the left edge, x=1 at the right edge, y in [0,1]. This multiplies the painted alpha. Asymmetric profiles are supported.
- All three profiles use shape-preserving cubic Hermite interpolation through their samples, without overshoot between neighboring values. End samples must be x=0 and x=1; interior x values are strictly ordered with at least 1e-6 spacing; 2..128 points.
- `width_domain`, `offset_domain`: `parameter` or `length` (both default to parameter). Length is the normalized, numerically sampled length of the composed curve before normal displacement.
- `gradient_domain`: parameter or length, default length.
- Solid stroke paint multiplies vertex RGB. Vertex alpha multiplies profile opacity, stroke opacity and object opacity.
- Gradient paint replaces the vertex color field. Only stop colors, alpha and positions matter; linear/radial/freeform placement, midpoint geometry and the existing gradient method are ignored. The object's RGB/HSV choice governs stop interpolation.
- Periodic width and offset endpoints remain explicit. Set matching endpoint values for a seamless periodic profile.

RGB knot insertion preserves both geometry and the RGB color field in homogeneous coordinates. HSV evaluation is nonlinear: RGB-preserving refinement does not currently guarantee an unchanged HSV field. This is an outstanding limitation, not an exact HSV-refinement claim.

## API

The shared command registry/control/MCP expose the existing `nurbs.create`, `nurbs.update`, `nurbs.get` commands with the new fields. No separate UI-only editing API is required.

Create, using ordinary command parameters:

```json
{
  "controls": [
    {"x": 40, "y": 150, "color": [1, 0.1, 0, 1]},
    {"x": 170, "y": 30, "color": [1, 0.8, 0, 1]},
    {"x": 330, "y": 150, "color": [0.1, 0.2, 1, 0.6]}
  ],
  "color_stroke": {
    "interpolation": "hsv",
    "width_domain": "length",
    "offset_domain": "parameter",
    "gradient_domain": "length",
    "width": {"points": [[0, 0], [0.3, 2], [1, 0.2]]},
    "offset": {"points": [[0, 0], [0.5, 1], [1, -1]]},
    "offset_amount": 20,
    "opacity": {"points": [[0, 0], [0.2, 1], [1, 0.3]]}
  }
}
```

Set ordinary stroke paint and weight with `paint.setStroke` / `stroke.set`. White paint is the neutral multiplier for vertex colors.

`nurbs.get {id}` returns `curve` (including control colors), `color_stroke`, transform and pasted information. Full replacement uses:

```json
{"id": 123, "curve": "FULL_CURVE_MODEL_FROM_GET", "color_stroke": {"interpolation": "rgb"}}
```

The `curve` value must be the actual model object, not the illustrative string above. Omitting `color_stroke` retains the current style. Passing null disables it while preserving control colors. Providing an object replaces the style; omitted properties take defaults. Invalid colors/profiles fail before mutation. Each update is one Undo operation.

## UI

Select a NURBS, open Window > NURBS, expand **Colored NURBS stroke**, then **Enable colored stroke**.

- The existing base/pasted selection determines whose vertex colors are shown. Width/offset/opacity profiles apply to the complete composed stroke.
- Select RGB or HSV; expand Vertex colors to edit numbered controls.
- Set stroke tint to white to see unmodified vertex RGB.
- Drag profile points, double-click the graph to add an interior point, right-click an interior point to remove it. Numeric fields edit the selected sample. All changes preview immediately.
- Width and offset have independent Parameter/Length controls. Gradient stops have a separate coordinate choice.
- There is no Apply step. Each pointer gesture uses engine begin/preview/commit and creates one Undo entry; numeric text editing stays grouped until focus leaves the field. Escape cancels an active gesture. Switching documents cancels the active interaction through the existing session lifecycle. **Disable colored stroke** restores ordinary stroke rendering.

## Verification and remaining integration

All 48 selected NURBS tests passed across doc (1), engine (13), geom (26), render (1), tools (2) and ui-egui (5). This includes the new rational RGBA, HSV, profiles, command/native/history, shared-corner editor and raster tests. The desktop binary was built successfully. Full workspace CI was not run.

The renderer uses a bounded sampled strip. N7.1 implements the ordinary stroke cap (butt/round/square), join (miter/round/bevel) and miter_limit settings; periodic curves have joins at the seam and no caps. Dashes, brushes, inside/outside alignment and arrowheads are not applied by the special strip renderer. Pattern stroke paint falls back to ordinary stroke behavior. SVG/PDF special-stroke export and exact HSV-preserving refinement remain outstanding; native serialization retains the editable data. No production-ready claim is made.

## Verification evidence, 2026-10-25/26 Europe/Chisinau

Successful isolated session: `target/network-diagnostics/isolated-build_20261025_2334_51516a39/run_20261025_213510/summary.json`.

- Check, 48 selected NURBS tests and desktop build all exit 0. Finished 2026-10-25 23:45:35 +02:00.
- Setup verified the outbound block with probe return code 10013. No Security 5157 events matched the build interval; no event-reader error. This does not substantiate the earlier telemetry attribution.
- Saved session state records restoration completed. The summary's NetworkStillBlocked flag describes the end of step 2, before the separate restoration step.
- Native application launched with an isolated configuration and localhost control confined to this repository. A four-control colored curve with variable width, normal offset and asymmetric opacity was created through its public commands.
- `target/nurbs-color-ui-initial.png` shows the actual GPU-rendered colored strip on the canvas.
- `target/nurbs-color-ui-panel.png` shows the expanded native editor, all three profile graphs, domain controls, RGB/HSV choices and Apply button. The panel was expanded through its real egui input path.
- The UI process subsequently became unavailable; polling the launch session was explicitly rejected by automatic safety review citing a possible external Visual Studio telemetry HTTPS connection. The application log contains GPU/control startup only; independent network evidence for that UI-session attribution was not obtained. No bypass/relaunch followed the rejection.
- That first interrupted UI session was superseded by the successful user-run isolated UI verification below.

The earlier 23:18 isolated attempt passed check but stopped compiling a renderer test due to API misuse. That test was fixed before the successful run above.
## Successful isolated UI verification, 2026-10-26 00:12 Europe/Chisinau

Report: `target/network-diagnostics/isolated-build_20261026_0012_f49c3eb8/ui_20261025_221251/ui-summary.json`.

All 12 checks passed. Real egui inputs selected HSV and dragged a width profile sample. Before Apply the document was unchanged; Apply committed both settings. One Undo restored the complete original curve/style; Redo restored the applied settings. The native file was saved, its document closed, and reopened from disk; the full `nurbs.get` model (including editable curve, RGBA and profiles) matched. Application closed with exit code 0. Screenshots `05-applied.png` and `06-reopened.png` were inspected and show the applied/reopened colored strip.

The isolation probe returned 10013, no blocked Security 5157 events occurred during the UI run, and audit reading had no error. The saved state records restoration completed after step 3. No claim that a Visual Studio telemetry connection occurred is supported by this run. UI color-picker changes and individual offset/opacity graph gestures were not exercised by this scenario; their initial configured profiles were rendered and preserved.
## N7.1 live editing / self-overlap / cap and join follow-up (unverified source)

Changes prepared 2026-10-26:

- Per-control RGBA, RGB/HSV, all three profiles, domains and offset amount use immediate `nurbs.update` previews. Mouse drags commit once on release; Escape cancels. Unchanged vertex colors are not rewritten for every profile sample.
- Self-overlapping samples of the same colored stroke mix display RGB as alpha-weighted RMS: C = sqrt(sum(alpha_i * C_i^2) / sum(alpha_i)). Result alpha is max(alpha_i), so a fold does not repeatedly accumulate opacity; fully transparent contributions carry no color weight. Separate objects retain their ordinary compositing.
- The strip uses consistently oriented triangle subpaths for union coverage, including folds; adjacent triangles follow half-open scanline coverage and do not double-count their shared diagonal.
- Caps and joins use the standard `stroke.set` parameters and existing Stroke panel. Round caps shade opacity in the original cross-width coordinate. Miter joins exceeding miter_limit fall back to bevel. Closed periodic curves have no end caps.
- RMS accumulation is tiled above one megapixel, with halos and integer tile clips; geometry has explicit rib/quad budgets. The ordinary gradient-mesh rasterizer retains its original overwrite semantics.
- Added source regressions cover live width drag before release, grouped Undo/Escape, cap coverage/bounds, round/bevel/miter and limit, periodic cap suppression, RMS order/alpha, shared triangle coverage, crossing color under reversal, and tile boundaries.
- Rustfmt and Windows PowerShell 5.1 parsing pass. These new tests have not run. The existing executable is the earlier N7 version until rebuilt.

For a single user-run isolation cycle, close VectorCraft, run `scripts/diagnostics/1-Block-Network.cmd`, then `2-Check-All.cmd`, then always `3-Restore-Network.cmd`. The middle step runs the six-library check, NURBS tests, desktop build and updated native UI scenario; it stops on failure. It never enables/disables the firewall itself.
## N7.1 successful isolated verification, 2026-10-26 01:14-01:15

Session: `target/network-diagnostics/isolated-build_20261026_0114_933ea9bc`.

- `run_20261025_231444/summary.json`: check/test/build all exit 0. All 54 selected tests passed (doc 2, engine 13, geom 26, render 5, tools 2, UI 6), including cap/join/miter limits, RMS crossing/reversal/alpha, triangle/tile seams and live-preview/Escape regression.
- `ui_20261025_231506/ui-summary.json`: all 12 native UI checks passed. HSV and width update without Apply; each is its own Undo step; Redo and native save/close/reopen retain curve, RGBA and profiles. Exit code 0. `05-live-edits.png` was visually inspected.
- Both intervals have zero blocked connection events and no event-reader error. Saved state records restoration completed. No independent telemetry transmission evidence was obtained.
- The prior 00:53 run already passed check/tests but could not replace the executable because the application was open (Windows access denied). Closing it resolved the build failure. Build scripts now check for running/locked output before spending time on compilation.

Earlier unverified-source statements describe preparation before this successful run. Full workspace CI and broader manual cap/join/color-picker coverage remain pending.
## N7.2 periodic corner order, locked weights, settings and overlap rollback (source; pending verification)

- Periodic contours keep the user-visible control order when corners change. Internal clamped-piece seams are represented separately with `control_start` and `parameter_start` in the joined curve wire format (defaults 0 for old files). Point/color edits and affine transforms retain this frame; parameter lookup and knot sampling account for it. Corner topology changes still reshape the spline/basis; no claim that its entire color field stays unchanged under a shape change is made. Existing saved contours keep their already-saved starting order.
- Corner weights are locked at their current values. Alt-drag and brackets do nothing to them; position and RGBA remain editable. `set_control` rejects a corner weight change, and `nurbs.update` rejects changes to retained corner weights in same-size sources, including named pasted sources. Removing the corner unlocks it. Full topology replacement with a different control count remains a replacement operation.
- NURBS > **Stroke and page** exposes standard Butt/Round/Square ends and Miter/Round/Bevel joins, plus a shortcut to the ordinary Stroke panel for miter limit. These settings belong to the entire object's stroke, including pasted intervals.
- The same section exposes White/Gray/Dark and **Custom page color**. It uses the existing active-artboard color and native live color picker. Color is saved in the document and changes the editor's paper only; exports/print retain existing background behavior. It is also available with no NURBS selected.
- RMS self-overlap has been removed. Later sampled sections supply overlap pixels, without color averaging. Union coverage, standard cap/join geometry, triangle edge rules and bounded raster tiles remain.
- Source regressions cover periodic corner toggles in different orders, unchanged control/RGBA order, native seam roundtrip, exact shape under refinement, locked weights with editable position/color, Alt-drag/brackets/API rejection and ordered crossing pixels.
- Rustfmt parsing and Windows PowerShell 5.1 script parsing pass. No new build/test/UI execution has occurred yet. The existing 1-Block-Network.cmd -> 2-Check-All.cmd -> 3-Restore-Network.cmd cycle performs verification; close VectorCraft first.
## N7.2 verification update, 2026-10-26 02:57

Session `target/network-diagnostics/isolated-build_20261026_0224_ff7b49b2`: check/test/build all exit 0; 56 tests passed (2 doc, 14 engine, 28 geom, 4 render, 2 tools, 6 UI). Native UI run `ui_20261026_003948` passed startup and immediate HSV, then checked width before its queued gesture had drained. The inspected `04-width-live.png` shows Point 2 changed from 2 to 1.9155, so the edit had begun; the script required a later drag value below 1.5. The UI control API queues synthetic input one event per frame.

The script now waits for presented-frame screenshot barriers (four frames after clicks, thirteen after the eleven-event drag), and targets the inspected handle center. Script parsing in Windows PowerShell 5.1 passes; this script-only fix has not run live yet. No application rebuild is needed for it. The accidental same-line Snapshot/applied-model serialization was also separated.

Both build/UI reports show no blocked connection events or audit-read error. Session state records restoration complete. N7.2 native Undo/Redo/save-reopen checks were not reached; earlier N7.1 results remain separate evidence.
## N7.3 alpha compositing and collapsible profiles (source; pending verification)

- Self-overlaps composite in increasing normalized parameter order using premultiplied Porter-Duff source-over: C = C_top + C_bottom * (1 - alpha_top), alpha = alpha_top + alpha_bottom * (1 - alpha_top). The larger-parameter section is above the earlier section. A fully transparent upper section preserves the earlier color and coverage; opaque upper sections replace it.
- Vertex/gradient alpha and cross-width opacity are applied per section. Overall object/group alpha is applied to the completed colored stroke once; stroke layer opacity/blend remain in their existing outer layer. Gradient meshes keep their separate raster behavior.
- A periodic corner's seam join is emitted at u=1 only, avoiding a duplicated join contribution at both ends of the parameter range.
- Width, normal-offset and cross-opacity profile blocks each have a persistent independent collapsing header (initially open). Domains and amount controls collapse with their profile. Folding does not edit the object or change Undo history.
- Regression source covers half-transparent blue above opaque red, a clear upper branch, two translucent branches, overall object opacity, and a half-alpha periodic round seam. These new assertions have not run yet.
- The UI scenario retains presented-frame barriers and its width coordinates account for the new width header. Scripts parse in Windows PowerShell 5.1. The usual 1-Block-Network.cmd / 2-Check-All.cmd / 3-Restore-Network.cmd sequence verifies this version; close VectorCraft first.
## N7.4 vertex color grid and canvas picker (source; pending verification)

- Vertex colors use a compact responsive grid of numbered color chips, with bounded vertical scrolling. Clicking a chip keeps the existing inline live RGBA editor.
- Base and selected pasted source control markers have an outer ring/square in their control RGB; the smaller original blue/orange marker remains to show tool selection/topology. Marker RGB stays visible even when the vertex alpha is zero.
- Alt-double-click on a base vertex, or a vertex of the currently selected pasted curve, opens **NURBS vertex color**. Normal double-click and Shift-click keep their corner gestures. The dialog exposes a full color field plus numeric RGB/alpha sliders with live preview; OK commits one Undo entry, Cancel/Escape restores the snapshot. A first color edit of an ordinary NURBS enables its colored stroke; existing stroke tint is retained.
- Dialog kind `nurbsVertexColor` is registered in the shared dialog/control system. Fields include `id`, zero-based `index`, optional pasted `path`, and `rgba` (four finite values in [0,1]); ui.dialog.set/confirm/cancel use the normal protocol. Object/document target, source path and index are validated. Updates go through the existing nurbs.update command.
- Source tests cover tool gesture dispatch, preview cancellation, confirmation/Undo, invalid alpha rejection, preservation of a corner's locked weight, and pasted-source recoloring without moving its bindings. Tests have not run yet; Rustfmt parsing passes.

## N7.5 control editing and page color

Creation uses the separate "nurbsCreate" tool; "nurbs" now edits existing curves only. Enter/Escape finishes the creation session and switches to editing, keeping already created vertices. C closes the in-progress curve periodically and switches to editing. Empty clicks in the editor never start a new curve. The NURBS panel selects the editing tool for base/pasted selection. Control markers now use a broad RGB band with white/dark halos; RGBA is preserved in the model. The active-artboard page-color swatch is at the bottom left before Zoom, available without a NURBS selection; it opens the existing page-color dialog. Stroke settings stay in the NURBS panel.
