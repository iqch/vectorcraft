# NURBS debris

A debris object binds editable source geometry to a native NURBS curve. Select
one base curve and one point-based source, then choose **Object > NURBS > Attach
NURBS Debris** (also available in **Window > NURBS**). If both selected objects
are NURBS, the panel lets you choose the base explicitly. The original source
stays in the document and is hidden by default; it is not consumed.

The NURBS panel provides:

- **Parameter**: the midpoint of the currently blended parameter interval.
- **u range**: remaps all phases through one affine parameter mapping; reversing
  the endpoints reverses the mapping. Local offsets remain unchanged.
- **Scale**: scales local tangent and normal offsets. **Offset** adds a normal
  displacement after scaling.
- **Blend**: selects/interpolates phases by their numeric keys, clamping beyond
  the first and last key.
- **Edit base / Edit source**, with links back from the source to its debris.
- **Add / replace**, **Move key**, **Copy key**, **Remove selected phase** and
  **Fix interpolated phase**. Fix creates a separate hidden source at the chosen
  key; an existing key is unchanged. Copy key retains the source link.

A parameter/scale/offset/blend/range gesture previews live, commits as one Undo,
and cancels with Escape. Deform the base or edit a linked source to update the
result. Source IDs, binding frames, local coordinates and phase settings persist
in native files. Source geometry is read from its linked object, rather than
copied into each phase. The generated children are derived output; use the source
controls to edit geometry.

## Geometry and phases

Each anchor stores `u`, tangent offset and normal offset. The binding uses the
nearest point on the composed curve, including pasted intervals. With a local
unit tangent T, left normal N and base evaluation C:

`P = C(mapped_u) + T * tangent_offset * scale + N * (normal_offset * scale + offset)`.

Both offset components are retained, including the residual at curve endpoints
and corners, so projecting and reconstructing the original point preserves it.
Periodic projections use the smallest covering unwrapped interval across the
seam. Open curves clamp evaluations to their endpoints. Arc-length ranges use a
sampled lookup; parameter ranges are the primary representation.

A phase captures the base curve as its binding reference. Later source edits are
reprojected against that reference, preserving subsequent deformation of the
base. This follows the Curve Engine attachment model while keeping phase art
linked. Separately moving both source and base changes both inputs.

Phases require equal vertex counts and matching topology (path subpaths and
handles, NURBS degree/knots/corners/periodicity, mesh dimensions, group structure
and masks). Replacing the only phase may change topology. Each neighboring phase
is materialized independently, then positions and weights interpolate in
homogeneous coordinates; NURBS/mesh point colors use HSV with the shortest hue
path. Ordinary appearance and raster pixels come from the first phase source.
Geometry phases do not crossfade bitmap textures or arbitrary paint stacks.

Supported sources: paths/live shape outlines, native NURBS, point-based groups
and compounds, gradient meshes, mesh envelopes, raster images and their opacity
mask art. Text should be outlined; other procedural objects should be expanded.
Pasted subtrees in phase NURBS must match. A source supports at most 16,384 point
samples (including handles/masks), and an object has at most 128 phases.

Missing/invalid base or phase source, incompatible edited topology, a degenerate
base, or a dependency cycle produces empty derived art and a warning in the
NURBS panel and Layers. Links remain available for repair. Undo restores them.
Generated debris children cannot be chosen as independent sources. Whole debris
objects can be linked as sources, with dependencies evaluated in order.

## Raster corners

An image is interpreted as four vertices (top-left, top-right, bottom-right,
bottom-left). Its derived result is a live 1x1 mesh envelope using VectorCraft's
existing raster warp, so no additional document object type is needed. Four
corners define the image warp; its interior is not separately projected to the
NURBS. Native persistence and standard SVG/PDF envelope expansion apply.

Raster warp now preserves an affine image directly. For non-affine RGB warps it
samples the mesh into one premultiplied-alpha image, eliminating compositing seams
between neighboring triangles. The derived raster is bounded to 2048 pixels on
its longest side; its sampling density aims to retain the source resolution and
at least two pixels per document point before that cap. This is an approximation
at high zoom. CMYK images retain the existing clip-based warp to preserve inks.

## Commands

| Command | Parameters |
|---|---|
| `nurbs.debris.create` | `base`, `source` or two selected objects; optional `name`, `parameter`, `scale=1`, `offset=0`, `hideSource=true` |
| `nurbs.debris.get` | `id` or selected debris; returns base, phases, blend, scale, offset, parameter, `u_range`, `l_range`, problem |
| `nurbs.debris.update` | `id`; optional `display` (`normal`/`mute`/`outline`), `base` (ID/null), `blend`, `scale`, `offset`, `parameter`, `u_range` or `l_range`, `mapping:[origin,span]` |
| `nurbs.debris.phase.add` | `id`, `source`, `phase`, `hideSource=true`; replaces an existing key |
| `nurbs.debris.phase.move` | `id`, `phase`, `newPhase`, `copy=false` |
| `nurbs.debris.phase.fix` | `id`, `phase`; creates an independent snapshot source; requires nonzero scale |
| `nurbs.debris.phase.remove` | `id`, `phase`; at least one phase must remain |
| `nurbs.debris.source` | `id`, optional `phase` (omit for base), `visible=true`, `select=visible` |

Malformed numeric data is rejected atomically. Broken relationships are valid
editable states. Native load regenerates cached children from the links.

Reference: [Curve Engine dev discussion](https://chatgpt.com/share/6adf67a4-a8c0-83ed-8363-75e441a24ce3),
including the final tangent/normal projection and phase interpolation corrections.
## Verification (2026-10-26)

- 93 filtered NURBS library tests passed: geometry 29, document 15, engine 26,
  renderer 13, egui 10. The new cases cover reversible projection/ranges, periodic
  seam intervals, source edits, homogeneous/HSV phase interpolation, topology
  rejection, phase fixing/copy/removal, Undo/cancel, native links and raster alpha.
- All 288 document model tests and five existing envelope renderer tests passed.
- After the Clippy cleanup, the seven document debris tests passed again.
- 27 checks passed in owned native application sessions, recorded in
  `target/nurbs-debris-native/state.json`. Actual Parameter dragging is one Undo;
  base-control dragging updates the result and its Undo restores a byte-identical
  PNG. Source navigation, phases, missing-base warnings, raster attachment,
  SVG/PDF export and native reopen were exercised. Screenshots visually confirm
  the red warning in the actual Layers row and the deformed translucent checker.
- At the initial debris milestone Region used `[[0,1],[1,1]]`; the later performance
  update changes the current default to `[[0,1],[1,0]]`. The earlier Linear and Default buttons
  were checked, including full alpha after returning to Default.
- The final rebuilt `target/debug/vectorcraft.exe` was launched separately, reopened
  the saved vector/raster debris scene and rendered it successfully. Both owned
  application sessions closed.
- Final offline desktop build and non-strict Clippy passed; the new debris and
  raster warp modules have no Clippy diagnostics.
- No full workspace CI or WASM build is claimed. Existing repository Clippy
  warnings outside the new modules remain.

Native evidence is in `target/nurbs-debris-native/`: `initial-panel.png`,
`parameter-drag.png`, `base-edit.png`, `phases.png`, `broken-layers.png`,
`raster.png`, `reopened.png`, `region-default.png`, `debris.vectorcraft`,
`export.svg` and `export.pdf`. The PNG pair `before-base-edit.png` /
`undo-base-edit.png` has identical hashes.

Display modes Normal, Mute and Outline are available per object and from the base
curve's Attached debris list. Geometry and links remain available in all modes;
see [NURBS performance](nurbs-performance.md) for behavior and release settings.
