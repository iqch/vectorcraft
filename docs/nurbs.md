# NURBS extension

Status: 2026-10-24 — N1 geometry implementation; application integration pending.

This custom branch adds editable NURBS to VectorCraft. It does not change the upstream
Illustrator-parity scores. The baseline desktop build was verified by the contributor.

## Agreed editing model

- Control points have positions and positive rational weights.
- A corner is the common clamped endpoint of two adjoining curves.
- Removing a corner merges their control polygons (common point once), rebuilds knots,
  and may change shape. The left endpoint weight wins if the endpoint weights differ.
- Creating, appending or removing controls selects degree `min(count - 1, 3)`.
- Splitting preserves shape, yielding two clamped curves with normalized domains.
- Refinement at a parameter preserves shape by modifying neighboring control points.

## N1 geometry API

`crates/geom/src/nurbs.rs`: `NurbsCurve` has private validated controls, knots and degree.
Construction accepts degrees 1–3, 2–4096 controls, finite coordinates of magnitude at
most 1e100, weights in 1e-100..=1e100, and clamped knots normalized to [0,1]. Interior
multiplicity cannot exceed degree. Invalid requests return `NurbsError`; edits return
new curves, leaving the source intact on failure.

`evaluate`, `set_control`, `append_control`, `remove_control`, `split` and `join` provide
geometry operations. `insert_knot` is a fixed-degree mathematical primitive.
`to_cubic` raises degree in homogeneous coordinates, preserving the parameterized curve.
`insert_vertex` raises low-degree curves to cubic before knot insertion, so the result
obeys automatic degree and preserves shape. This can add more than one control point:
a degree-one or degree-two curve cannot generally gain an interior knot, raise degree,
and retain the same control count increment as fixed-degree knot insertion. The future
UI must expose this accurately. Full-multiplicity knots refuse further insertion;
splitting at those parameters remains available.

Split returns independent curves with identical shared controls. The document-level
corner model must keep their endpoint edits synchronized; it is not implemented yet.
Closed/periodic curves, closest-parameter search, drawing, selection, native storage,
engine commands, Undo/Redo and UI tools remain future stages. No NURBS tool ships yet.

## Verification

Tests include an exact rational quarter circle, degree changes, repeated insertion,
parameter-preserving splits, join endpoints/counts, degree elevation, common weight
scaling, invalid input, and randomized rational refinement/splitting.

Run `cargo test -p vectorcraft-geom --lib nurbs`, followed by `cargo xtask ci` before commit.
Verified: native library build, WASM library check, workspace formatting and workspace Clippy with warnings denied. All seven NURBS tests passed through Cargo, including randomized rational refinement and splitting (2026-10-24). All 79 geometry tests also passed via the built test executable on 2026-10-25. Dependencies are now cached. Full workspace CI validation remains pending; the resumed run on 2026-10-25 has passed formatting and Clippy and is building tests with one Cargo job to limit transient disk use.

Mathematical reference (original implementation; no source code copied):
https://pages.mtu.edu/~shene/COURSES/cs3621/NOTES/spline/NURBS-knot-insert.html

## Revision history

- 2026-10-24: N1 initial geometry and regression tests; document/UI integration pending.

Validation note (2026-10-25): resuming the CI process was blocked by automatic approval review reporting an incidental untrusted telemetry connection with unspecified destination/data. Full workspace test results remain unverified. Only local log inspection and direct geometry test execution were continued.

## N2 command and native storage — 2026-10-25

Implemented and compiled in the contributor-run frozen/offline test build: validated serde roundtrip for NurbsCurve;
LiveShape::Nurbs stores source geometry, a local display outline and an affine transform;
`nurbs.create` accepts controls [{x,y,weight?}] and closed?, creates a path through the
existing art/history pipeline. closed means straight endpoint closure, not periodic NURBS.
Ordinary anchor editing follows existing live-shape expansion behavior; dedicated NURBS
control editing remains future work. Rational display spans use bounded homogeneous
subdivision at 0.05 document-point tolerance; polynomial spans remain exact cubics.
Preview accuracy is in local coordinates and scales with the object transform.

Added tests (all passed in the contributor-run test build): validated serialization, polynomial/rational display
outline accuracy, command creation, native format roundtrip and Undo/Redo. The new
integration check was blocked during dependency checking by automatic security review
reporting an unspecified external telemetry connection. No successful compilation or
execution of N2 tests is claimed. Formatting and diff whitespace checks passed.

### Confirmed test result — 2026-10-25

The contributor executed the targeted frozen/offline test command and the shared log
(target/nurbs-tests.log) records successful compilation in 31m 38s, followed by
1 passing engine test (create, native save/load, Undo/Redo) and all 9 geometry NURBS
tests passing. The earlier N2 compilation/test blocker is resolved for these targets.
This does not establish a full workspace CI pass, a desktop UI build, visual validation,
or the presence/absence of network attempts; the Cargo log contains no firewall audit.

## N3 interactive tool — draft, 2026-10-25

Implemented in source: NURBS toolbar entry in Basic and Advanced layouts, click creation,
control-polygon overlays, control dragging with preview/commit/cancel, Alt-drag and
bracket-key weight adjustment, Delete/Backspace control removal, C straight-end closure,
Enter completion, Escape cancellation, and Ctrl/Cmd-click shape-preserving insertion
using bounded screen-space parameter search. Existing NURBS objects must first be
selected with Selection, then edited with the NURBS tool. A dedicated corner editor,
periodic closure and split-at-click interaction are not implemented.

`nurbs.update` preserves appearance and transforms and uses the engine history path.
Added engine interaction regressions for creation, move/weight gestures, cancellation,
Undo/Redo and atomic rejection of invalid curve data; these are not yet verified.

Verification: two tool tests compiled and ran successfully using rustc directly and
Rust's LLVM linker (rust-lld); both passed. The tool library also compiled directly
against a consistent set of cached dependencies. These checks do not replace Cargo
workspace validation. The engine test type check was stopped by automatic approval
review reporting an external Visual Studio telemetry connection, including when run
without any linker. VSCMD_SKIP_SENDTELEMETRY=1 did not resolve the reported block.
No desktop build or visual check of N3 has completed. The tool is not declared ready
for user testing. Source changes remain uncommitted.

Validation update, 2026-10-25 03:31 Europe/Chisinau: contributor explicitly authorized
continuation with the reported Visual Studio telemetry connection. Automatic approval
review nevertheless rejected resuming the Cargo process, acknowledging that consent
but requiring authorization of a specified payload that is not exposed by the tool.
The reduced-debug targeted Cargo test build did not reach a test summary. No additional
test pass or desktop executable is claimed. Cargo clean of the engine dev artifacts
succeeded, freeing 6.8 GiB; source and downloaded dependencies were preserved.

### Confirmed N3 targeted verification — 2026-10-25

The contributor-run Cargo test command completed in 4m 54s. All 14 selected tests
passed: 3 engine, 9 geometry and 2 tool tests. Engine coverage includes interactive
click creation, control dragging, weight adjustment, Escape rollback, Undo/Redo,
atomic rejection of invalid data, and native save/reopen. This supersedes the earlier
unverified status for those tests. Desktop compilation and visual validation remain
pending. The test log does not establish any network or telemetry behavior.

### Desktop build and initial visual verification — 2026-10-25

Contributor-run desktop build succeeded in 20m 12s (unoptimized dev profile), producing
target/debug/vectorcraft.exe. Agent launched this executable with a workspace-local
APPDATA profile and localhost control port 7987. Actual UI inspection showed the NURBS
tool present in the Basic toolbar; selecting it and five click/up pairs created one
editable NURBS object. document.inspect reported kind NURBS, and a presented-frame
screenshot (target/nurbs-ui-initial.png) confirmed the visible curve, five control points,
control polygon and weight label on the canvas. Initial UI creation is visually verified.

Further UI drag/weight checks could not complete: the control endpoint became unavailable,
and resuming the app process was rejected by automatic approval review reporting the
Visual Studio telemetry connection. The app log contains no recorded panic or cause of
termination. Do not attribute the endpoint loss to a product crash without more evidence.
Drag, weight, cancellation, native persistence and history remain covered by the passing
engine/tool tests; they are not all claimed as visually verified. Full workspace CI is
still pending. The compiled initial interactive tool can be tested manually; corner
editing and click-to-split remain future features.

## N4 corners and periodic curves — implemented, integration validation pending

Status: 2026-10-25. Source now stores `NurbsPath`: either a single clamped/periodic
kernel or clamped pieces sharing corner controls. Old single-curve native data still
loads through the legacy representation. A corner shares position and weight on both
sides; moving it updates both endpoint records. Clearing a corner joins its neighboring
control polygons and may change shape. Each piece selects degree from its control count,
up to cubic. Topology changes (corner and periodic switches) can change shape.

Smooth periodic kernels use a cyclic control polygon and an extended periodic knot
sequence. Independent controls are stored once. Knot insertion refines three translated
periods and keeps one period; display uses exact endpoint clamping before the existing
bounded approximation. Low-degree periodic curves can be elevated for shape-preserving
insertion. Degree one remains only C0; simple cubic periodic knots provide C2 continuity.

Commands: `nurbs.create` now accepts `periodic` and `corners` (control indices);
`nurbs.corner {id,index}` toggles a shared corner; `nurbs.periodic {id,periodic?}` sets
or toggles periodic topology. Legacy `closed` remains straight closure for old data.
`nurbs.update` retains source topology and rebuilds the display path. C now toggles
periodic topology, rather than straight closure. K, Shift-click or a double-click on a
control toggles a corner. Corners and open endpoints are squares, smooth controls are
circles, and a periodic control polygon includes its last-to-first edge.

Verified independently with rustc + LLVM against the actual geom crate root: all 89
geometry tests pass, including 17 NURBS tests. Coverage includes independent uniform
cubic basis values, seam position/tangent, exact clamping, low-degree elevation, shared
corner position/weight, removal/join, wraparound corners, outline error, legacy loading,
serde topology roundtrip, and randomized periodic refinement including the seam.
A minimized proptest case is retained under crates/geom/proptest-regressions: repeated
seam knots required a general insertion span beyond the finite control-array boundary.
The corrected algorithm validates its affected control range instead of rejecting the span.

Added but not yet executed: engine command/gesture, native save/reopen and Undo/Redo
regressions for periodic and corner states. Cargo validation was again stopped by the
execution environment's Visual Studio telemetry refusal despite previously granted
consent. No full integration pass, N4 desktop build or visual validation is claimed.
The previous desktop binary still contains N3 until rebuilt.

Mathematical references (original implementation):
- https://pages.mtu.edu/~shene/COURSES/cs3621/NOTES/spline/B-spline/bspline-curve.html
- https://pages.mtu.edu/~shene/COURSES/cs3621/NOTES/spline/NURBS-knot-insert.html

## N4 integration and desktop build verified

2026-10-25: user-run frozen Cargo validation completed successfully. All 25 targeted
NURBS tests passed: 6 engine, 17 geometry and 2 tools. This includes corner/periodic
commands, gestures, shared weights, native save/reopen and Undo/Redo. The native desktop
build also finished successfully; target/debug/vectorcraft.exe now contains N4.
Evidence: target/nurbs-topology-tests.log and target/nurbs-topology-build.log.
Manual visual verification of the new corner and periodic gestures remains pending;
full workspace CI has not been run for N4.

## N4 shortcut routing correction

2026-10-25: the idle UI text shortcut handler now dispatches letters claimed by the
active tool before single-key command shortcuts and tool switching. This fixes NURBS
C being intercepted by the Cut tool and routes K through the same path. Existing
physical-key mapping supports non-Latin layouts; repeated key events do not toggle.
Added a headless UI regression covering C, repeat, Russian layout and unclaimed V.
Cargo execution was rejected by automatic review of incidental Visual Studio telemetry;
the new UI test and refreshed desktop build remain unverified. Source diff check passes.

## N5 pasted NURBS, stage one

Group conversion and the model/editing API are implemented; no new UI in this stage.
See [Pasted NURBS API](nurbs-pasted-api.md) for ordering, geometry, persistence and limits.
All 96 geometry tests passed in the independent Rust/LLVM run. Engine integration tests
and desktop build remain pending at this checkpoint.

N5 validation update, 2026-10-25: final frozen Cargo run passed all 35 targeted tests
(9 engine, 24 geom, 2 tools); the desktop build succeeded. Live control API smoke checks
passed for group conversion, ignored art, range/scale update, full-model create, removal
and Undo. Native save completed without warnings, and the rendered test contour showed
both pasted intervals correctly. UI panel/selection remains stage two; full workspace
CI was not run. See docs/nurbs-pasted-api.md and target/nurbs-paste-*.log/json/png.

## N6 panel and pasted selection, verified

2026-10-25: Window > NURBS now exposes group conversion, the pasted hierarchy, activity
checkboxes, selection and a two-handle range strip. Periodic parents show 0..2, including
the seam at 1; endpoint/interval drags are one undo step and Escape cancels. Numeric
Start/End/Scale edits apply together with Apply. Canvas pasted mode selects a contour,
shows its name and parent ghost, and drags its interval; Delete removes it. Nested edits
use the live posed parent frame. Selection/drafts are bound to the current document.

Pasted records inherit source node names with deterministic unique suffixes (unnamed:
curve-ID). Existing saved record names remain unchanged. Duplicate creates an independent,
initially disabled copy at the same range; inactive pastes remain editable but do not
replace or reserve their base interval. New API: nurbs.paste.duplicate and enabled on
nurbs.paste.update / nurbs.get. Optional serialized enabled defaults true for legacy files.

Validation passed 97 geometry tests, 43 targeted Cargo tests and native desktop build.
Live UI checked creation, names, duplicate, disabled-copy selection and movement, enable,
strip movement into the second turn, one-step Undo and saved/reopened activity flags.
See [NURBS UI](nurbs-ui.md) and [Pasted API](nurbs-pasted-api.md). Full workspace CI pending.

## N7 - colored NURBS stroke (draft, 2026-10-26)

RGBA controls, RGB/HSV interpolation, width/normal-offset/cross-opacity profiles, API and editor are implemented. User-run OS-isolated check, all 48 selected NURBS tests and desktop build passed. Native isolated UI verification passed HSV selection, width graph drag, draft/Apply, Undo/Redo and native save/close/reopen; the resulting screenshots were inspected. Export and advanced-stroke integration remain incomplete. See [colored NURBS status and API](nurbs-color.md).
N7.1 follow-up, 2026-10-26: source now implements live panel previews with gesture Undo/Escape, alpha-weighted RMS self-overlap, ordinary cap/join/miter-limit settings, union coverage and bounded raster tiles. New compile/tests/native verification are pending; earlier passing evidence is for N7. See nurbs-color.md.
N7.1 verification update, 2026-10-26 01:17: isolated check, 54 selected tests, desktop build and 12 native UI checks passed. Live HSV/width, Undo/Redo and native save/reopen verified. Caps/joins and RMS/tile regressions pass; no parity score change. See nurbs-color.md for session evidence and remaining limitations.

N7.2 source update, 2026-10-26 02:17: stable periodic control/color order and serialized internal seam, locked corner weights, NURBS Stroke and page settings with existing artboard paper color, and removal of RMS overlap mixing. New compile/tests/native verification pending; prior N7.1 results do not cover these changes. No parity score change. See nurbs-color.md.

N7.2 verification update, 2026-10-26 02:57: isolated check, 56 selected tests and desktop build passed. Native HSV passed; width check raced queued input. Screenshot confirms partial live width update. Script-only frame-wait fix prepared; native remainder pending. See nurbs-color.md. No parity score change.

N7.3 source update, 2026-10-26 03:18: larger-parameter stroke sections composite above earlier sections with source-over alpha; object alpha applies once; periodic seam join is not doubled. Width/offset/opacity blocks now collapse independently. New checks/build/native verification pending; no parity score change. See nurbs-color.md.

N7.4 source update, 2026-10-26 03:29: numbered vertex-color grid, control-color marker outlines, Alt-double-click nurbsVertexColor dialog for base/selected pasted controls, live RGBA preview and commit/cancel through nurbs.update. Compile/tests/native verification pending, together with N7.3 alpha/collapsing changes. No parity score change. See nurbs-color.md.

### 2026-10-26 N7.5 source update

Separate `nurbsCreate` (create) and `nurbs` (edit) tools, Enter/Escape handoff, stronger RGB control bands with contrast halos, and a page-background swatch before the status-bar Zoom control. N7.3/N7.4 check/59-tests/build and 12 native UI checks passed in `isolated-build_20261026_0331_fd065d82`; restoration completed, no blocked events recorded. N7.5 formatting and script syntax/plan pass; its build/native verification remains pending. No parity-score change. See `docs/nurbs-ui.md`.