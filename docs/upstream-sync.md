# Updating the NURBS fork

`origin` is `iqch/vectorcraft`; `upstream` is `storytold/vectorcraft`.
Keep `main` as an upstream mirror and `feature/nurbs` as the custom version.
Use a separate `integration/upstream-YYYYMMDD` branch for each update, so the
last checked custom version remains available while resolving and testing.

## Update procedure

1. Commit the current source changes on `feature/nurbs`, and push that branch.
   Keep local `.codex` settings, build outputs and machine-specific diagnostic
   scripts outside the source commit.
2. Run `git fetch upstream`, then create an integration branch from
   `feature/nurbs`.
3. Run `git merge --no-commit --no-ff upstream/main` on the integration branch.
4. Resolve conflicts and port our additions to the upstream modules. When a
   large file has been split, its removed code often belongs in a new child
   module. Preserve both upstream behavior and the custom additions.
5. Stage the resolutions and run the checks below. Create the merge commit
   after they pass, and push the integration branch.
6. Fast-forward `feature/nurbs` to that checked merge commit and push it.
   Fast-forward `main` to `upstream/main` separately; it contains upstream only.

The merge commit retains both parent histories. The original custom commit can
always be checked out again. No force push is needed for this process.

## Checks

```sh
cargo check --locked -p vectorcraft
cargo test --locked --lib -p vectorcraft-geom -p vectorcraft-doc -p vectorcraft-tools -p vectorcraft-render -p vectorcraft-engine -p vectorcraft-ui-egui nurbs
cargo test --locked -p vectorcraft-ui-egui --lib shortcuts::
cargo test --locked -p vectorcraft-render --lib live::tests::
cargo build --release --locked -p vectorcraft
```

The release profile keeps opt-level 3, fat LTO, one codegen unit and no debug
information. A compiler job limit does not limit raster worker threads.
The Windows executable is `target/release/vectorcraft.exe`.

The application updater follows the official `storytold/vectorcraft` releases.
Update this custom build through Git: installing an official release installs
the upstream executable, which does not contain our NURBS additions.

Check new/existing native documents, curve editing, pasted curves, phases,
Region Default, individual debris display modes and SVG/PDF export when testing
the desktop executable. Check NURBS C/K keys alongside the shared keymap, including
held-key repeats and a non-Latin keyboard layout.

## Module moves in upstream 0.17

The first integration targets upstream `eb0e99a5` and the custom commit
`49447e97`. The nine textual conflicts came from these module splits:

| Former location | Current location for our additions |
| --- | --- |
| `doc/src/live.rs` | `doc/src/live/expand.rs`: raster warp |
| `doc/src/node.rs` | `node/live_shape.rs`, `extras.rs`, `wire.rs`, `ops.rs`: geometry, persistence and bounds |
| `engine/src/lib.rs` | `session/edit.rs`: live Region/debris refresh after edits |
| `render/src/fx.rs` | `fx/entries.rs`: image-aware live cache inputs |
| `render/src/lib.rs` | `lib.rs`, `renderer.rs`, `renderer/paint.rs`, `threads.rs`: strip cache and shared worker policy |
| `ui-egui/src/chrome.rs` | `chrome/status_bar.rs`: artboard background control |
| `ui-egui/src/menus.rs` | `menus/tree.rs`: NURBS commands and panel |
| `ui-egui/src/panels/layers.rs` | `layers/model.rs`, `rows.rs`: problem labels and tooltips |
| `ui-egui/src/shortcuts.rs` | `shortcuts.rs`, `shortcuts/tests.rs`: tool-owned keys before keymap dispatch |

Paths in the table are under `crates/`. Preserve the upstream once-per-frame
shortcut guard when routing the NURBS panel's Escape and tool-owned letters.
