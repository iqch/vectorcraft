# Pasted NURBS API (custom branch, stage one)

Implemented 2026-10-25 from the user-supplied Matisse `nurbs.py` model.
No new panel or pointer mode in this stage. Commands use the existing engine registry
and control protocol (`engine.execute`); discover them with `engine.commands`.

## Group conversion

`nurbs.paste {id?: groupId, mode?: "u" | "p" | "l", scale?: number}`

Omitting `id` uses the single selected group. The group must be an ordinary non-clipping
group. Only its direct NURBS children participate. The first NURBS child in the stored
bottom-to-top child order becomes the base; click/selection order is irrelevant.
Other NURBS children must be open (clamped), as in Matisse. Non-NURBS children, including
nested groups, stay untouched. At least two NURBS members are required.

The base keeps its ID, appearance and live affine transform. Child geometry is converted
into base-local coordinates before attachment. Consumed NURBS children become named
records inside the base model and their separate scene nodes are removed. Names inherit the source node label, with unique numeric suffixes for duplicates; unnamed
sources use `curve-<original node ID>`. Their old appearances are not applied to the composite.
The group and all non-NURBS objects remain. The result selects the base.

Returns `{id: baseId, group: groupId, pasted: [name, ...], ignored: [nodeId, ...]}`.
The whole conversion is one undo step. Invalid geometry, overlaps, locked participating
members or unsupported children fail without partially consuming the group.

Example control request (a document with group 42 must already exist):

```json
{"id":1,"method":"engine.execute","params":{"command":"nurbs.paste","params":{"id":42,"mode":"u","scale":1}}}
```

## Model and editing API

| Command | Parameters | Result |
| --- | --- | --- |
| `nurbs.get` | `{id}` | `{id, curve, transform: [a,b,c,d,e,f], pasted: [{name,mode,scale,enabled,u_range,l_range}]}` |
| `nurbs.create` | `{curve: fullModel}` OR `{controls: [{x,y,weight?}], corners?, periodic?}` | `{id}` |
| `nurbs.update` | `{id, curve: fullModel, closed?}` | `{id}` |
| `nurbs.paste.update` | `{id,name,u_range?: [start,end],l_range?: [start,end],scale?,enabled?}` | `{id}` |
| `nurbs.paste.duplicate` | `{id,name,newName?}` | `{id,name,enabled:false}` |
| `nurbs.paste.remove` | `{id,name}` | `{id}` |

Read a model with `nurbs.get` and send its `curve` field to `nurbs.create` or
`nurbs.update` to create or replace the complete composite, including nested pastes.
These full-model paths do not require a scene group. `curve` cannot be combined with
`controls`, `corners` or `periodic` in `nurbs.create`.

`nurbs.paste.update` and `.remove` address a direct child by its stable name. Edit nested
children through the complete model for now. `u_range` and `l_range` are alternatives;
passing both is an error. Activity can be changed with `enabled`; disabled records retain their
data but do not replace the base or reserve an interval. Duplicates start disabled. Ranges must be increasing and keep a nonzero span; the original
child direction is retained. Adjacent ranges may touch but may not overlap. Removing
a paste restores the underlying base interval. Scale may be zero or negative.
All edits support Undo/Redo and native document persistence.

```json
{"id":2,"method":"engine.execute","params":{"command":"nurbs.paste.update","params":{"id":17,"name":"curve-23","u_range":[0.25,0.45],"scale":1.5}}}
```

## Geometry semantics

A paste replaces an interval of its parent's *base* contour, rather than adding another
painted object. Both endpoints project onto the parent. Each child control stores a
coordinate along the base and tangent/normal offsets. Internal controls whose projections
fall outside the endpoint interval use the corresponding endpoint frame. Materialization
recomputes positions from the current base frames, preserving weights and knots; nested
children follow recursively. Base control/weight edits and knot refinement retain pasted
records and regenerate the composite. An edit that makes attachments invalid is rejected.

Modes: `u` normalized parameter; `p` raw parameter (the Rust domain is already [0,1], so
these two modes coincide); `l` base-local arc length. Ranges returned by `nurbs.get` refer
to the undeformed base, not the composite length or world-space scale. Initial periodic
attachment chooses the shortest endpoint interval. Unwrapped ranges may cross the seam,
e.g. `[0.9,1.1]`. Opening such a base is rejected until the seam-crossing paste is removed
or moved. A periodic paste cannot cover a full turn.

The wire model uses the existing single-curve representation for simple curves, and
`{pieces, periodic, pasted}` for composites. Each pasted record has
`{name,curve,mode,anchors:[{coordinate,tangent,normal}],scale}`. Endpoints must have zero
offsets; anchor count must match the child control count. Old single-curve files remain
readable. `NurbsPath::evaluate` evaluates the base for editing/projection;
`evaluate_composed` evaluates the visible composite.

Projection uses a bounded knot-aware search, frames use numerical tangents and length
binding uses a 32-subdivision-per-knot-span chord table. They approximate the Matisse
semantics rather than promising bit-identical samples. Visible outlines preserve exact
trimmed rational source pieces and use the existing bounded NURBS-to-Bezier conversion.
Limits: 128 direct pasted records, nesting depth 8, 16,384 controls in the full tree,
32,768 output path elements. Degenerate frames and nonfinite data are rejected.

## Validation

All 96 geometry tests passed via direct Rust/LLVM compilation of the actual geometry
crate. Seven new pasted tests cover replacement, frame following, modes, range/scale
editing, reversed direction, overlap and malformed wire rejection, refinement retention,
periodic seams, nested contours, affine transforms, independent rational circle frames
and resource limits. Final frozen Cargo validation passed all 35 targeted NURBS tests:
9 engine, 24 geometry and 2 tools. Engine coverage includes group order, ignoring other
art, transforms, atomic failure, read/create/update, native persistence and Undo/Redo.
The native desktop build succeeded. A live loopback control-protocol smoke test in an
isolated profile verified group conversion (two pasted curves plus an ignored rectangle),
range/scale update, full-model creation, removal and Undo. The application rendered both
replaced intervals correctly and saved the native test document without warnings.
Evidence: target/nurbs-paste-tests.log, target/nurbs-paste-build.log,
target/nurbs-paste-api-smoke.json, target/nurbs-paste-api-preview.png and
target/nurbs-paste-api-smoke.vectorcraft. Full workspace CI was not run. No new panel or
pasted-selection pointer mode is included; those remain stage two.


Stage-two UI: see [NURBS panel](nurbs-ui.md), including the periodic 0..2 strip, names, duplicate and enable controls.
