//! Canvas selection and interval dragging of pasted curves. Tool-local selection is
//! shared with the panel through options and is never persisted in the document.
use super::*;
use kurbo::ParamCurveNearest;
use serde_json::Value;

#[derive(Default)]
pub(super) struct PasteEditor {
    pub enabled: bool,
    document: Option<u64>,
    pub target: Option<NodeId>,
    pub path: Vec<String>,
    drag: Option<Drag>,
}
struct Drag {
    id: NodeId,
    document: u64,
    root: NurbsCurve,
    parent: NurbsCurve,
    parent_path: Vec<String>,
    name: String,
    xf: Affine,
    start: f64,
    last: f64,
    range: [f64; 2],
}
impl PasteEditor {
    pub fn busy(&self) -> bool {
        self.drag.is_some()
    }
    pub fn options(&self) -> Value {
        json!({"pastedMode":self.enabled,"pastedDocument":self.document,"pastedTarget":self.target.map(|v|v.0),"pastedPath":self.path})
    }
    pub fn set_option(&mut self, key: &str, value: &Value) {
        if self.drag.is_some() {
            return;
        }
        match key {
            "pastedDocument" => self.document = value.as_u64(),
            "pastedMode" => self.enabled = value.as_bool().unwrap_or(false),
            "pastedTarget" => {
                self.target = value.as_u64().map(NodeId);
                self.path.clear();
            }
            "pastedPath" => {
                self.path = value
                    .as_array()
                    .filter(|a| a.len() <= 8)
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
            }
            _ => {}
        }
    }
    pub fn valid<'a>(&self, cx: &'a ToolContext) -> Option<(NodeId, &'a NurbsCurve, Affine)> {
        let (id, c, xf) = active(cx)?;
        if self.document != Some(cx.revision.0) || self.target != Some(id) {
            return None;
        }
        c.source_at(&self.path)?;
        Some((id, c, xf))
    }
    fn hit(root: &NurbsCurve, xf: Affine, p: Point, tol: f64, preferred: &[String]) -> Option<Vec<String>> {
        fn visit(
            c: &NurbsCurve,
            xf: Affine,
            p: Point,
            tol: f64,
            path: &mut Vec<String>,
            best: &mut Option<(f64, Vec<String>)>,
            depth: usize,
            preferred: &[String],
        ) {
            if depth >= 8 {
                return;
            }
            for paste in c.pastes() {
                let Ok(child) = c.materialize_paste(&paste.name) else { continue };
                path.push(paste.name.clone());
                if !paste.enabled && !preferred.starts_with(path.as_slice()) {
                    path.pop();
                    continue;
                }
                if let Ok(mut outline) = child.to_bezpath(0.05) {
                    outline.apply_affine(xf);
                    let distance = outline.segments().map(|s| s.nearest(p, 1e-5).distance_sq).fold(f64::INFINITY, f64::min).sqrt();
                    if distance <= tol
                        && best.as_ref().is_none_or(|(d, old)| {
                            distance < *d - 1e-6 || (distance <= *d + 1e-6 && (path.len() > old.len() || path.as_slice() == preferred))
                        })
                    {
                        *best = Some((distance, path.clone()));
                    }
                }
                visit(&child, xf, p, tol, path, best, depth + 1, preferred);
                path.pop();
            }
        }
        let mut best = None;
        visit(root, xf, p, tol, &mut vec![], &mut best, 0, preferred);
        best.map(|(_, path)| path)
    }
    pub fn cursor(&self, cx: &ToolContext, p: Point) -> Cursor {
        if self.drag.is_some() || active(cx).is_some_and(|(_, c, xf)| Self::hit(c, xf, p, cx.tol(8.0), &self.path).is_some()) {
            Cursor::Move
        } else {
            Cursor::Arrow
        }
    }
    pub fn pointer(&mut self, cx: &ToolContext, e: &PointerEvent) -> Vec<Action> {
        if self.drag.as_ref().is_some_and(|d| d.document != cx.revision.0) {
            self.drag = None;
            return vec![Action::Cancel];
        }
        if e.mods.alt && matches!(e.kind, PointerKind::Down | PointerKind::DoubleClick) {
            if let Some((id, root, xf)) = self.valid(cx) {
                if let Ok(child) = root.materialize_at(&self.path) {
                    if let Some((index, _)) = child.controls().iter().enumerate().find(|(_, p)| (xf * p.position).distance(e.pos) <= cx.tol(8.0)) {
                        let path = self.path.clone();
                        let mut actions = if self.drag.take().is_some() { vec![Action::Cancel] } else { vec![] };
                        if e.kind == PointerKind::DoubleClick {
                            actions.push(Action::Dialog("nurbsVertexColor".into(), json!({"id":id.0,"index":index,"path":path})));
                        }
                        return actions;
                    }
                }
            }
        }
        match e.kind {
            PointerKind::Down => {
                let Some((id, c, xf)) = active(cx) else { return vec![] };
                if self.document != Some(cx.revision.0) {
                    self.path.clear();
                }
                let Some(path) = Self::hit(c, xf, e.pos, cx.tol(8.0), &self.path) else {
                    self.path.clear();
                    self.document = Some(cx.revision.0);
                    self.target = Some(id);
                    return vec![];
                };
                self.document = Some(cx.revision.0);
                self.target = Some(id);
                self.path = path.clone();
                let Some((name, parent_path)) = path.split_last() else { return vec![] };
                let Ok(parent) = c.materialize_at(parent_path) else { return vec![] };
                let Ok((range, _)) = parent.paste_ranges(name) else { return vec![] };
                let Some((u, _)) = nearest(&parent, xf, e.pos) else { return vec![] };
                self.drag = Some(Drag {
                    id,
                    document: cx.revision.0,
                    root: c.clone(),
                    parent,
                    parent_path: parent_path.to_vec(),
                    name: name.clone(),
                    xf,
                    start: u,
                    last: u,
                    range,
                });
                vec![Action::Begin("Move Pasted NURBS".into())]
            }
            PointerKind::Drag => {
                let Some(d) = &mut self.drag else { return vec![] };
                let Some((mut u, _)) = nearest(&d.parent, d.xf, e.pos) else { return vec![] };
                if d.parent.is_periodic() {
                    u += (d.last - u).round();
                }
                d.last = u;
                let mut delta = u - d.start;
                if !d.parent.is_periodic() {
                    delta = delta.clamp(-d.range[0], 1.0 - d.range[1]);
                }
                let range = [d.range[0] + delta, d.range[1] + delta];
                let Ok(parent) = d.parent.update_paste(&d.name, Some(range), None) else { return vec![] };
                let Ok(root) = d.root.replace_source_at(&d.parent_path, parent) else { return vec![] };
                vec![Action::Preview("nurbs.update".into(), json!({"id":d.id.0,"curve":root,"closed":false}))]
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
    pub fn key(&mut self, cx: &ToolContext, key: ToolKey) -> Vec<Action> {
        if key == ToolKey::Escape {
            if self.drag.take().is_some() {
                return vec![Action::Cancel];
            }
            self.path.clear();
            self.enabled = false;
            return vec![];
        }
        if self.drag.is_some() {
            return vec![];
        }
        if matches!(key, ToolKey::Delete | ToolKey::Backspace) {
            let Some((id, root, _)) = self.valid(cx) else { return vec![] };
            let Some((name, parent_path)) = self.path.split_last() else { return vec![] };
            let Ok(parent) = root.materialize_at(parent_path) else { return vec![] };
            let Ok(parent) = parent.remove_paste(name) else { return vec![] };
            let Ok(root) = root.replace_source_at(parent_path, parent) else { return vec![] };
            self.path.clear();
            return vec![update(id, &root)];
        }
        vec![]
    }
    pub fn overlays(&self, cx: &ToolContext) -> Vec<Overlay> {
        let Some((_, root, xf)) = self.valid(cx) else { return vec![] };
        let mut out = vec![];
        let parent_path = if self.path.is_empty() { &[][..] } else { &self.path[..self.path.len() - 1] };
        if let Ok(parent) = root.materialize_at(parent_path) {
            if let Ok(base) = parent.with_pastes(vec![]) {
                if let Ok(mut path) = base.to_bezpath(0.05) {
                    path.apply_affine(xf);
                    out.push(Overlay::Path { path, color: [130, 140, 155], width: 1.0, dashed: true });
                }
            }
        }
        if !self.path.is_empty() {
            if let Ok(child) = root.materialize_at(&self.path) {
                if let Ok(mut path) = child.to_bezpath(0.05) {
                    path.apply_affine(xf);
                    out.push(Overlay::Path { path, color: [245, 135, 35], width: 2.5, dashed: false });
                }
                for (i, p) in child.controls().iter().enumerate() {
                    let at = xf * p.position;
                    let tint = std::array::from_fn(|ch| (p.color[ch].clamp(0.0, 1.0) * 255.0).round() as u8);
                    if child.is_corner(i) || i == 0 || i + 1 == child.controls().len() {
                        super::control_marker(&mut out, at, tint, true);
                        out.push(Overlay::Anchor { p: at, color: [245, 135, 35], filled: false, size: 7.0 });
                    } else {
                        super::control_marker(&mut out, at, tint, false);
                        out.push(Overlay::CurvePoint { p: at, color: [245, 135, 35], filled: false, size: 7.0 });
                    }
                }
                if let (Ok(a), Some(name)) = (child.evaluate(0.0), self.path.last()) {
                    out.push(Overlay::Label { p: xf * a, text: name.clone(), color: [245, 135, 35] });
                }
            }
        }
        out
    }
}
