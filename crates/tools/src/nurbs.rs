//! Separate NURBS creation and control-editing tools. All document changes go through engine commands.
use crate::{Action, Cursor, Mods, Overlay, PointerEvent, PointerKind, Tool, ToolContext, ToolKey};
use serde_json::json;
use vectorcraft_doc::{LiveShape, NodeId, NodeKind};
use vectorcraft_geom::nurbs::{ControlPoint, NurbsPath as NurbsCurve};
use vectorcraft_geom::{Affine, Point};

#[path = "nurbs/pasted.rs"]
mod pasted;

#[derive(Default)]
pub struct NurbsTool {
    create: bool,
    paste_edit: pasted::PasteEditor,
    first: Option<Point>,
    drawing: bool,
    selected: Option<usize>,
    drag: Option<(NodeId, usize, NurbsCurve, Affine, Point, bool)>,
}
fn active<'a>(cx: &'a ToolContext<'_>) -> Option<(NodeId, &'a NurbsCurve, Affine)> {
    if cx.selection.objects.len() != 1 {
        return None;
    }
    let id = *cx.selection.objects.first()?;
    let n = cx.doc.node(id)?;
    let NodeKind::Path { live: Some(live), .. } = &n.kind else { return None };
    let LiveShape::Nurbs { curve, xf, .. } = &**live else { return None };
    Some((id, curve, *xf))
}
fn update(id: NodeId, curve: &NurbsCurve) -> Action {
    Action::Exec("nurbs.update".into(), json!({"id":id.0,"curve":curve}))
}
/// Bounded screen-space search, refined around the best sampled knot interval.
fn nearest(curve: &NurbsCurve, xf: Affine, p: Point) -> Option<(f64, f64)> {
    let mut best = (f64::INFINITY, 0.5, 0.0, 1.0);
    for pair in curve.knots().windows(2) {
        let (Some(&a), Some(&b)) = (pair.first(), pair.get(1)) else { continue };
        if b <= a {
            continue;
        }
        for i in 0..=16 {
            let u = a + (b - a) * i as f64 / 16.0;
            let distance = (xf * curve.evaluate(u).ok()?).distance(p);
            if distance < best.0 {
                best = (distance, u, (u - (b - a) / 16.0).max(a), (u + (b - a) / 16.0).min(b));
            }
        }
    }
    let (mut lo, mut hi) = (best.2, best.3);
    for _ in 0..36 {
        let a = lo + (hi - lo) / 3.0;
        let b = hi - (hi - lo) / 3.0;
        if (xf * curve.evaluate(a).ok()?).distance(p) < (xf * curve.evaluate(b).ok()?).distance(p) { hi = b } else { lo = a }
    }
    let u = (lo + hi) * 0.5;
    Some((u, (xf * curve.evaluate(u).ok()?).distance(p)))
}
impl NurbsTool {
    pub fn create() -> Self {
        Self { create: true, ..Default::default() }
    }
    fn create_pointer(&mut self, cx: &ToolContext, e: &PointerEvent) -> Vec<Action> {
        if e.kind != PointerKind::Down {
            return vec![];
        }
        if self.drawing {
            if let Some((id, curve, xf)) = active(cx) {
                if xf.determinant().abs() >= 1e-12 {
                    if let Ok(new) = curve.append_control(ControlPoint::new(xf.inverse() * e.pos, 1.0)) {
                        return vec![update(id, &new)];
                    }
                }
            }
            return vec![];
        }
        if let Some(first) = self.first.take() {
            self.drawing = true;
            vec![Action::Exec("nurbs.create".into(), json!({"controls":[{"x":first.x,"y":first.y},{"x":e.pos.x,"y":e.pos.y}]}))]
        } else {
            self.first = Some(e.pos);
            vec![]
        }
    }
}
/// A three-pixel color band with light/dark halos, legible over any artwork.
fn control_marker(out: &mut Vec<Overlay>, at: Point, tint: [u8; 3], square: bool) {
    for (size, color) in [(17.0, [255, 255, 255]), (15.0, [20, 20, 20]), (13.0, tint)] {
        out.push(if square {
            Overlay::Anchor { p: at, color, filled: true, size }
        } else {
            Overlay::CurvePoint { p: at, color, filled: true, size }
        });
    }
}
impl Tool for NurbsTool {
    fn id(&self) -> &'static str {
        if self.create { "nurbsCreate" } else { "nurbs" }
    }
    fn options(&self) -> serde_json::Value {
        self.paste_edit.options()
    }
    fn set_option(&mut self, key: &str, value: &serde_json::Value) {
        if self.create || self.drag.is_some() {
            return;
        }
        self.paste_edit.set_option(key, value);
        if self.paste_edit.enabled {
            self.first = None;
            self.drawing = false;
            self.selected = None;
        }
    }
    fn deactivate(&mut self, _: &ToolContext) -> Vec<Action> {
        let busy = self.drag.take().is_some() || self.paste_edit.busy();
        self.paste_edit = Default::default();
        self.first = None;
        self.drawing = false;
        if busy { vec![Action::Cancel] } else { vec![] }
    }
    fn cursor(&self, cx: &ToolContext, p: Point, _: Mods) -> Cursor {
        if self.paste_edit.enabled {
            return self.paste_edit.cursor(cx, p);
        }
        if self.create { Cursor::Crosshair } else { Cursor::ArrowHollow }
    }
    fn busy(&self) -> bool {
        self.drag.is_some() || self.first.is_some() || self.paste_edit.busy()
    }
    fn claims_key(&self, _: &ToolContext, key: ToolKey) -> bool {
        matches!(
            key,
            ToolKey::Enter
                | ToolKey::Escape
                | ToolKey::Delete
                | ToolKey::Backspace
                | ToolKey::BracketLeft
                | ToolKey::BracketRight
                | ToolKey::Letter('c')
                | ToolKey::Letter('k')
        )
    }
    fn pointer(&mut self, cx: &ToolContext, e: &PointerEvent) -> Vec<Action> {
        if self.create {
            return self.create_pointer(cx, e);
        }
        if self.paste_edit.enabled {
            return self.paste_edit.pointer(cx, e);
        }
        match e.kind {
            PointerKind::Down => {
                if let Some((id, curve, xf)) = active(cx) {
                    if e.mods.ctrl || e.mods.cmd {
                        if let Some((u, distance)) = nearest(curve, xf, e.pos) {
                            if distance <= cx.tol(10.0) {
                                if let Ok(new) = curve.insert_vertex(u) {
                                    self.selected = None;
                                    return vec![update(id, &new)];
                                }
                            }
                        }
                        return vec![];
                    }
                    if let Some((index, _)) = curve.controls().iter().enumerate().find(|(_, p)| (xf * p.position).distance(e.pos) <= cx.tol(8.0)) {
                        self.first = None;
                        self.selected = Some(index);
                        if e.mods.shift {
                            self.selected = None;
                            self.drawing = false;
                            self.first = None;
                            return vec![Action::Exec("nurbs.corner".into(), json!({"id":id.0,"index":index}))];
                        }
                        if xf.determinant().abs() < 1e-12 {
                            return vec![];
                        }
                        if e.mods.alt && curve.is_corner(index) {
                            return vec![];
                        }
                        self.drag = Some((id, index, curve.clone(), xf, e.pos, e.mods.alt));
                        return vec![Action::Begin("Edit NURBS control".into())];
                    }
                }
                self.selected = None;
                if let Some(hit) = vectorcraft_doc::hit::hit_test(cx.doc, e.pos, cx.hit_options()) {
                    if let Some(node) = cx.doc.node(hit.leaf) {
                        if matches!(&node.kind, NodeKind::Path { live: Some(live), .. } if matches!(&**live, LiveShape::Nurbs { .. })) {
                            return vec![Action::Exec("select.set".into(), json!({"ids":[hit.leaf.0]}))];
                        }
                    }
                }
                vec![]
            }
            PointerKind::DoubleClick => {
                if let Some((id, curve, xf)) = active(cx) {
                    if let Some((index, _)) = curve.controls().iter().enumerate().find(|(_, p)| (xf * p.position).distance(e.pos) <= cx.tol(8.0)) {
                        self.first = None;
                        self.drawing = false;
                        self.selected = None;
                        let mut actions = if self.drag.take().is_some() { vec![Action::Cancel] } else { vec![] };
                        if e.mods.alt {
                            actions.push(Action::Dialog("nurbsVertexColor".into(), json!({"id":id.0,"index":index})));
                        } else {
                            actions.push(Action::Exec("nurbs.corner".into(), json!({"id":id.0,"index":index})));
                        }
                        return actions;
                    }
                }
                vec![]
            }
            PointerKind::Drag => {
                let Some((id, index, curve, xf, start, weight)) = &self.drag else { return vec![] };
                let Some(mut control) = curve.controls().get(*index).copied() else { return vec![] };
                if *weight && curve.is_corner(*index) {
                    return vec![];
                }
                if *weight {
                    control.weight = (control.weight * ((start.y - e.pos.y) * cx.zoom / 80.0).exp()).clamp(0.001, 1000.0);
                } else {
                    control.position = xf.inverse() * e.pos;
                }
                if let Ok(new) = curve.set_control(*index, control) {
                    return vec![Action::Preview("nurbs.update".into(), json!({"id":id.0,"curve":new}))];
                }
                vec![]
            }
            PointerKind::Up => {
                if self.drag.take().is_some() {
                    vec![Action::Commit]
                } else {
                    vec![]
                }
            }
            _ => vec![],
        }
    }
    fn key(&mut self, cx: &ToolContext, key: ToolKey, _: Mods) -> Vec<Action> {
        if self.paste_edit.enabled {
            return self.paste_edit.key(cx, key);
        }
        if self.create {
            if matches!(key, ToolKey::Enter | ToolKey::Escape) {
                self.first = None;
                self.drawing = false;
                return vec![Action::SwitchTool("nurbs".into())];
            }
            if self.drawing && key == ToolKey::Letter('c') {
                if let Some((id, _, _)) = active(cx) {
                    self.drawing = false;
                    return vec![Action::Exec("nurbs.periodic".into(), json!({"id":id.0})), Action::SwitchTool("nurbs".into())];
                }
            }
            return vec![];
        }
        if key == ToolKey::Escape {
            self.first = None;
            self.drawing = false;
            return if self.drag.take().is_some() { vec![Action::Cancel] } else { vec![] };
        }
        if self.create || self.drag.is_some() {
            return vec![];
        }
        if key == ToolKey::Enter {
            self.first = None;
            self.drawing = false;
            return vec![];
        }
        let Some((id, curve, _)) = active(cx) else { return vec![] };
        if key == ToolKey::Letter('c') {
            self.drawing = false;
            self.first = None;
            self.selected = None;
            return vec![Action::Exec("nurbs.periodic".into(), json!({"id":id.0}))];
        }
        if key == ToolKey::Letter('k') {
            let Some(index) = self.selected.take() else { return vec![] };
            self.drawing = false;
            return vec![Action::Exec("nurbs.corner".into(), json!({"id":id.0,"index":index}))];
        }
        let Some(index) = self.selected else { return vec![] };
        let new = match key {
            ToolKey::Delete | ToolKey::Backspace => curve.remove_control(index),
            ToolKey::BracketLeft | ToolKey::BracketRight => {
                if curve.is_corner(index) {
                    return vec![];
                }
                let Some(mut p) = curve.controls().get(index).copied() else { return vec![] };
                p.weight = (p.weight * if key == ToolKey::BracketRight { 1.2 } else { 1.0 / 1.2 }).clamp(0.001, 1000.0);
                curve.set_control(index, p)
            }
            _ => return vec![],
        };
        if let Ok(new) = new {
            self.selected = Some(index.min(new.controls().len() - 1));
            vec![update(id, &new)]
        } else {
            vec![]
        }
    }
    fn overlays(&self, cx: &ToolContext) -> Vec<Overlay> {
        if self.paste_edit.enabled {
            return self.paste_edit.overlays(cx);
        }
        let mut out = vec![];
        let color = [75, 140, 240];
        if let Some(p) = self.first {
            out.push(Overlay::Anchor { p, color, filled: true, size: 7.0 });
        }
        if let Some((_, curve, xf)) = active(cx).filter(|_| !self.create || self.drawing) {
            let mut previous = None;
            for (i, p) in curve.controls().iter().enumerate() {
                let at = xf * p.position;
                if let Some(a) = previous {
                    out.push(Overlay::Line { a, b: at, color, dashed: true });
                }
                let endpoint = !curve.is_periodic() && (i == 0 || i + 1 == curve.controls().len());
                let tint = std::array::from_fn(|ch| (p.color[ch].clamp(0.0, 1.0) * 255.0).round() as u8);
                control_marker(&mut out, at, tint, curve.is_corner(i) || endpoint);
                if curve.is_corner(i) || endpoint {
                    out.push(Overlay::Anchor { p: at, color, filled: self.selected == Some(i), size: 7.0 });
                } else {
                    out.push(Overlay::CurvePoint { p: at, color, filled: self.selected == Some(i), size: 7.0 });
                }
                if self.selected == Some(i) {
                    out.push(Overlay::Label {
                        p: at,
                        text: if curve.is_corner(i) {
                            "Corner: weight locked / Alt-double-click: color".into()
                        } else {
                            format!("w = {:.3} / Alt-drag / [ ] / Alt-double-click: color", p.weight)
                        },
                        color,
                    });
                }
                previous = Some(at);
            }
        }
        if let Some((_, curve, xf)) = active(cx).filter(|_| !self.create || self.drawing) {
            if curve.is_periodic() {
                if let (Some(a), Some(b)) = (curve.controls().last(), curve.controls().first()) {
                    out.push(Overlay::Line { a: xf * a.position, b: xf * b.position, color, dashed: true });
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{cx, paint};
    use vectorcraft_doc::{Appearance, Document, Node, Selection};
    use vectorcraft_geom::PathData;
    #[test]
    fn nurbs_create_with_clicks_and_finish_in_edit_tool() {
        let doc = Document::new(400.0, 400.0);
        let selection = Selection::default();
        let paint = paint();
        let cx = cx(&doc, &selection, &paint);
        let mut t = NurbsTool::create();
        assert!(t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 10.0, 10.0)).is_empty());
        assert!(t.busy());
        let actions = t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 100.0, 100.0));
        assert!(matches!(actions.first(),Some(Action::Exec(id,_))if id=="nurbs.create"));
        assert_eq!(t.key(&cx, ToolKey::Enter, Mods::default()), vec![Action::SwitchTool("nurbs".into())]);
        assert!(!t.busy());
    }
    #[test]
    fn nurbs_edit_empty_clicks_never_start_creation() {
        let doc = Document::new(400.0, 400.0);
        let selection = Selection::default();
        let paint = paint();
        let cx = cx(&doc, &selection, &paint);
        let mut edit = NurbsTool::default();
        assert_eq!(edit.id(), "nurbs");
        assert_eq!(edit.cursor(&cx, Point::ZERO, Mods::default()), Cursor::ArrowHollow);
        for (x, y) in [(20.0, 20.0), (150.0, 90.0), (300.0, 200.0)] {
            assert!(edit.pointer(&cx, &PointerEvent::new(PointerKind::Down, x, y)).is_empty());
            assert!(edit.pointer(&cx, &PointerEvent::new(PointerKind::Up, x, y)).is_empty());
            assert!(!edit.busy());
        }
    }
    #[test]
    fn control_drag_previews_and_escape_rolls_back() {
        let curve = NurbsCurve::from_controls(vec![ControlPoint::new(Point::new(10.0, 10.0), 1.0), ControlPoint::new(Point::new(100.0, 100.0), 1.0)])
            .unwrap();
        let preview = PathData::from_bezpath(&curve.to_bezpath(0.05).unwrap());
        let mut doc = Document::new(400.0, 400.0);
        let id = doc.alloc_id();
        let mut n = Node::path(id, preview.clone(), Appearance::default());
        n.kind = NodeKind::Path {
            path: preview.clone(),
            live: Some(Box::new(LiveShape::Nurbs { curve, preview, xf: Affine::IDENTITY, color_stroke: None })),
            rule: Default::default(),
            clipping: false,
            guide: false,
        };
        doc.insert(None, 0, n).unwrap();
        let mut selection = Selection::default();
        selection.set([id]);
        let paint = paint();
        let cx = cx(&doc, &selection, &paint);
        let mut create = NurbsTool::create();
        assert_eq!(create.id(), "nurbsCreate");
        assert!(create.pointer(&cx, &PointerEvent::new(PointerKind::Down, 10.0, 10.0)).is_empty(), "create must not grab existing vertices");
        assert!(create.pointer(&cx, &PointerEvent::new(PointerKind::Drag, 25.0, 30.0)).is_empty());
        assert!(
            matches!(create.pointer(&cx, &PointerEvent::new(PointerKind::Down, 40.0, 60.0)).first(),Some(Action::Exec(id,_))if id=="nurbs.create")
        );
        let mut t = NurbsTool::default();
        assert!(t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 300.0, 300.0)).is_empty());
        assert!(t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 350.0, 300.0)).is_empty());
        assert!(!t.busy(), "selected curve must not turn empty clicks into creation");
        assert!(matches!(t.pointer(&cx, &PointerEvent::new(PointerKind::Down, 10.0, 10.0)).first(), Some(Action::Begin(_))));
        assert!(matches!(t.pointer(&cx,&PointerEvent::new(PointerKind::Drag,25.0,30.0)).first(),Some(Action::Preview(id,_))if id=="nurbs.update"));
        assert_eq!(t.key(&cx, ToolKey::Escape, Mods::default()), vec![Action::Cancel]);
        let picker = t.pointer(&cx, &PointerEvent::new(PointerKind::DoubleClick, 10.0, 10.0).with_mods(Mods { alt: true, ..Default::default() }));
        assert!(matches!(picker.last(),Some(Action::Dialog(kind,p)) if kind=="nurbsVertexColor" && p["index"]==0));
        assert!(t.pointer(&cx, &PointerEvent::new(PointerKind::Up, 25.0, 30.0)).is_empty());
    }
}
