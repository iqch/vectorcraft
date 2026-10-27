//! Live debris: source-linked point clouds bound to composed NURBS frames.
use crate::{Document, LiveShape, Node, NodeId, NodeKind};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};
use vectorcraft_color::Color;
use vectorcraft_geom::{
    Affine, PathData, Point,
    nurbs::{
        NurbsPath,
        debris::{DebrisAnchor, DebrisFrames},
    },
};
pub mod cloud;
use cloud::{Cloud, Sample};
pub const MAX_POINTS: usize = 16384;
pub const MAX_PHASES: usize = 128;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DisplayMode {
    #[default]
    Normal,
    Mute,
    Outline,
}
impl DisplayMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Mute => "Mute",
            Self::Outline => "Outline",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Phase {
    pub key: f64,
    pub source: NodeId,
    /// Only the binding curve is retained. The phase art always comes from source.
    pub reference: NurbsPath,
    pub bind_scale: f64,
    pub bind_offset: f64,
    pub bind_map: [f64; 2],
    #[serde(default)]
    pub anchors: Vec<DebrisAnchor>,
    #[serde(default)]
    pub signature: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Wire", into = "Wire")]
pub struct Debris {
    pub base: Option<NodeId>,
    pub phases: Vec<Phase>,
    pub blend: f64,
    pub scale: f64,
    pub offset: f64,
    pub map: [f64; 2],
    pub display: DisplayMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    base: Option<NodeId>,
    phases: Vec<Phase>,
    blend: f64,
    scale: f64,
    offset: f64,
    map: [f64; 2],
    #[serde(default)]
    display: DisplayMode,
    #[serde(default)]
    error: Option<String>,
}
impl TryFrom<Wire> for Debris {
    type Error = String;
    fn try_from(w: Wire) -> Result<Self, String> {
        let finite = |v: f64| v.is_finite() && v.abs() <= 1e6;
        if !(1..=MAX_PHASES).contains(&w.phases.len())
            || ![w.blend, w.scale, w.offset, w.map[0], w.map[1]].into_iter().all(finite)
            || w.map[1].abs() < 1e-12
        {
            return Err("invalid debris parameters or phase count (1..128)".into());
        }
        if w.phases.windows(2).any(|p| p[0].key >= p[1].key)
            || w.phases.iter().any(|p| {
                !finite(p.key)
                    || ![p.bind_scale, p.bind_offset, p.bind_map[0], p.bind_map[1]].into_iter().all(finite)
                    || p.bind_map[1].abs() < 1e-12
                    || p.anchors.len() > MAX_POINTS
                    || p.anchors.iter().any(|a| ![a.u, a.tangent, a.normal].into_iter().all(|v| v.is_finite() && v.abs() <= 1e12))
            })
        {
            return Err("phase keys must be finite and distinct; invalid debris bindings".into());
        }
        Ok(Self { base: w.base, phases: w.phases, blend: w.blend, scale: w.scale, offset: w.offset, map: w.map, display: w.display, error: w.error })
    }
}
impl From<Debris> for Wire {
    fn from(s: Debris) -> Self {
        Self { base: s.base, phases: s.phases, blend: s.blend, scale: s.scale, offset: s.offset, map: s.map, display: s.display, error: s.error }
    }
}
/// Curves are evaluated in document space, including their composed pasted intervals.
pub fn base_curve(doc: &Document, id: NodeId) -> Result<NurbsPath, String> {
    let n = doc.node(id).ok_or("base NURBS curve is missing")?;
    let NodeKind::Path { live: Some(live), .. } = &n.kind else { return Err("base is not a native NURBS curve".into()) };
    let LiveShape::Nurbs { curve, xf, .. } = &**live else { return Err("base is not a native NURBS curve".into()) };
    curve.transformed(*xf).map_err(|e| e.to_string())
}
fn bindings(phase: &mut Phase, cloud: &Cloud) -> Result<(), String> {
    let signature = cloud.signature();
    if phase.signature == signature && phase.anchors.len() == cloud.points.len() {
        return Ok(());
    }
    let frames = DebrisFrames::new(&phase.reference).map_err(|e| e.to_string())?;
    let count = cloud.points.len().max(1) as f64;
    let center = Point::new(cloud.points.iter().map(|p| p.p.x / count).sum(), cloud.points.iter().map(|p| p.p.y / count).sum());
    let around = frames.nearest(center).map_err(|e| e.to_string())?;
    let mut anchors = cloud
        .points
        .iter()
        .map(|p| frames.project(p.p, around, phase.bind_scale, phase.bind_offset).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    if phase.reference.is_periodic() && anchors.len() > 1 {
        let mut u: Vec<_> = anchors.iter().map(|a| a.u.rem_euclid(1.0)).collect();
        u.sort_by(f64::total_cmp);
        u.dedup();
        if u.len() > 1 {
            let mut gap = (-1.0, 0.0);
            for i in 0..u.len() {
                let a = *u.get(i).ok_or("parameter missing")?;
                let b =
                    if i + 1 == u.len() { u.first().copied().ok_or("parameter missing")? + 1.0 } else { *u.get(i + 1).ok_or("parameter missing")? };
                if b - a > gap.0 {
                    gap = (b - a, b.rem_euclid(1.0));
                }
            }
            let start = gap.1;
            let mid = start + (1.0 - gap.0) * 0.5;
            let turns = (around - mid).round();
            for a in &mut anchors {
                let u = a.u.rem_euclid(1.0);
                a.u = u + if u < start { 1.0 } else { 0.0 } + turns;
            }
        }
    }
    for a in &mut anchors {
        a.u = (a.u - phase.bind_map[0]) / phase.bind_map[1];
    }
    phase.anchors = anchors;
    phase.signature = signature;
    Ok(())
}
fn phase_range(p: &Phase) -> Option<[f64; 2]> {
    let mut range = [f64::INFINITY, f64::NEG_INFINITY];
    for a in &p.anchors {
        range[0] = range[0].min(a.u);
        range[1] = range[1].max(a.u);
    }
    range.into_iter().all(f64::is_finite).then_some(range)
}
impl Debris {
    pub fn new(doc: &Document, base: NodeId, source: NodeId) -> Result<Self, String> {
        let mut d = Self {
            base: Some(base),
            phases: vec![],
            blend: 0.0,
            scale: 1.0,
            offset: 0.0,
            map: [0.0, 1.0],
            display: DisplayMode::Normal,
            error: None,
        };
        d.add_phase(doc, source, 0.0)?;
        Ok(d)
    }
    pub fn add_phase(&mut self, doc: &Document, source: NodeId, key: f64) -> Result<(), String> {
        if !key.is_finite() || key.abs() > 1e6 {
            return Err("phase key must be finite".into());
        }
        let curve = base_curve(doc, self.base.ok_or("base is not assigned")?)?;
        let cloud = Cloud::of(doc.node(source).ok_or("phase source is missing")?)?;
        if let Some(first) = self.phases.iter().find(|p| p.key != key) {
            let first = Cloud::of(doc.node(first.source).ok_or("first phase source is missing")?)?;
            if cloud.vertices != first.vertices || cloud.topology != first.topology {
                return Err("phase sources must have the same vertex count and compatible topology".into());
            }
        }
        let mut phase = Phase {
            key,
            source,
            reference: curve,
            bind_scale: self.scale,
            bind_offset: self.offset,
            bind_map: self.map,
            anchors: vec![],
            signature: 0,
        };
        bindings(&mut phase, &cloud)?;
        if let Some(old) = self.phases.iter_mut().find(|p| p.key == key) {
            *old = phase;
        } else {
            if self.phases.len() >= MAX_PHASES {
                return Err("too many debris phases".into());
            }
            self.phases.push(phase);
            self.phases.sort_by(|a, b| a.key.total_cmp(&b.key));
        }
        Ok(())
    }
    pub fn selected_phases(&self) -> Result<(usize, usize, f64), String> {
        let first = self.phases.first().ok_or("debris has no phases")?;
        if self.blend <= first.key {
            return Ok((0, 0, 0.0));
        }
        let last = self.phases.len() - 1;
        if self.blend >= self.phases.get(last).ok_or("phase missing")?.key {
            return Ok((last, last, 0.0));
        }
        let right = self.phases.partition_point(|p| p.key < self.blend).min(last);
        let b = self.phases.get(right).ok_or("phase missing")?;
        if b.key == self.blend {
            return Ok((right, right, 0.0));
        }
        let left = right.saturating_sub(1);
        let a = self.phases.get(left).ok_or("phase missing")?;
        Ok((left, right, (self.blend - a.key) / (b.key - a.key)))
    }
    pub fn range(&self) -> Result<[f64; 2], String> {
        let (l, r, t) = self.selected_phases()?;
        let a = phase_range(self.phases.get(l).ok_or("phase missing")?).ok_or("phase has no points")?;
        let b = phase_range(self.phases.get(r).ok_or("phase missing")?).ok_or("phase has no points")?;
        Ok(std::array::from_fn(|i| self.map[0] + self.map[1] * (a[i] * (1.0 - t) + b[i] * t)))
    }
    pub fn parameter(&self) -> Result<f64, String> {
        let r = self.range()?;
        Ok((r[0] + r[1]) * 0.5)
    }
    pub fn set_parameter(&mut self, u: f64) -> Result<(), String> {
        if !u.is_finite() || u.abs() > 1e4 {
            return Err("invalid attachment parameter".into());
        }
        self.map[0] += u - self.parameter()?;
        Ok(())
    }
    pub fn set_range(&mut self, range: [f64; 2]) -> Result<(), String> {
        if !range.into_iter().all(|v| v.is_finite() && v.abs() <= 1e4) || (range[1] - range[0]).abs() < 1e-12 {
            return Err("debris range must be finite and nonzero".into());
        }
        let old = self.range()?;
        let span = old[1] - old[0];
        if span.abs() < 1e-12 {
            return Err("the projected object has zero parameter span; move it by parameter instead".into());
        }
        let scale = (range[1] - range[0]) / span;
        self.map = [range[0] + (self.map[0] - old[0]) * scale, self.map[1] * scale];
        Ok(())
    }
    pub fn dependencies(&self) -> Vec<NodeId> {
        self.base.into_iter().chain(self.phases.iter().map(|p| p.source)).collect()
    }
    pub fn reproject_sources(&mut self, doc: &Document) -> Result<Vec<Cloud>, String> {
        let mut clouds = vec![];
        for p in &mut self.phases {
            let cloud = Cloud::of(doc.node(p.source).ok_or_else(|| format!("Phase {} source {} is missing", p.key, p.source))?)?;
            if let Some(first) = clouds.first() {
                let first: &Cloud = first;
                if cloud.vertices != first.vertices || cloud.topology != first.topology {
                    return Err("phase vertex count or topology no longer matches".into());
                }
            }
            bindings(p, &cloud)?;
            clouds.push(cloud);
        }
        Ok(clouds)
    }
    pub fn build(&mut self, doc: &Document) -> Result<Node, String> {
        let curve = base_curve(doc, self.base.ok_or("base curve is not assigned")?)?;
        let frames = DebrisFrames::new(&curve).map_err(|e| e.to_string())?;
        let clouds = self.reproject_sources(doc)?;
        let (l, r, t) = self.selected_phases()?;
        let a = self.phases.get(l).ok_or("phase missing")?;
        let b = self.phases.get(r).ok_or("phase missing")?;
        let ca = clouds.get(l).ok_or("phase missing")?;
        let cb = clouds.get(r).ok_or("phase missing")?;
        let mut points = Vec::with_capacity(ca.points.len());
        for (i, left) in ca.points.iter().enumerate() {
            let right = cb.points.get(i).ok_or("phase point mismatch")?;
            let pa = frames.materialize(*a.anchors.get(i).ok_or("binding missing")?, self.map, self.scale, self.offset).map_err(|e| e.to_string())?;
            let pb = frames.materialize(*b.anchors.get(i).ok_or("binding missing")?, self.map, self.scale, self.offset).map_err(|e| e.to_string())?;
            points.push(Sample {
                p: Point::new(
                    (pa.x * left.weight * (1.0 - t) + pb.x * right.weight * t) / (left.weight * (1.0 - t) + right.weight * t),
                    (pa.y * left.weight * (1.0 - t) + pb.y * right.weight * t) / (left.weight * (1.0 - t) + right.weight * t),
                ),
                weight: left.weight * (1.0 - t) + right.weight * t,
                color: vectorcraft_geom::nurbs::mix_color(left.color, right.color, t, true),
            });
        }
        let source = doc.node(self.phases.first().ok_or("phase missing")?.source).ok_or("source missing")?;
        cloud::materialize(source, &points)
    }
}
/// Outline geometry is separate from the retained normal output used by other bindings.
fn outline_art(n: &Node) -> Node {
    let mut out = n.clone();
    outline_node(&mut out);
    out.visible = true;
    out
}
fn outline_node(n: &mut Node) {
    use crate::Appearance;
    use vectorcraft_color::Paint;
    use vectorcraft_geom::Shape;
    n.opacity = 1.0;
    n.blend = Default::default();
    n.extras_mut().mask = None;
    n.appearance = Appearance::basic(Paint::None, Paint::solid(Color::BLACK), 1.0).into();
    match &mut n.kind {
        NodeKind::Path { live, clipping, .. } => {
            *live = None;
            *clipping = false;
        }
        NodeKind::Image(im) => {
            let mut path = PathData::from_bezpath(&vectorcraft_geom::Rect::new(0.0, 0.0, im.width as f64, im.height as f64).to_path(0.01));
            path.transform(im.xf);
            n.kind = NodeKind::Path { path, live: None, rule: Default::default(), clipping: false, guide: false };
        }
        NodeKind::Mesh(mesh) => {
            n.kind = NodeKind::Path { path: mesh.outline(), live: None, rule: Default::default(), clipping: false, guide: false };
        }
        NodeKind::Group { children, clip } => {
            *clip = false;
            for c in children {
                outline_node(Arc::make_mut(c));
            }
        }
        NodeKind::Compound { children, .. } | NodeKind::Envelope { content: children, .. } => {
            for c in children {
                outline_node(Arc::make_mut(c));
            }
        }
        _ => {}
    }
}
pub fn allocate(n: &mut Node, doc: &mut Document) {
    n.id = doc.alloc_id();
    if let Some(c) = n.children_mut() {
        for n in c {
            allocate(Arc::make_mut(n), doc);
        }
    }
    if let Some(m) = n.extras_mut().mask.as_mut() {
        allocate(Arc::make_mut(&mut m.art), doc);
    }
}
/// Dependencies are evaluated before users. Cycles become empty, diagnosed groups.
pub fn refresh(before: &Document, doc: &mut Document) {
    let mut records = BTreeMap::new();
    doc.walk(|n| {
        if let Some(d) = &n.extras().nurbs_debris {
            records.insert(n.id, (**d).clone());
        }
    });
    if records.is_empty() {
        return;
    }
    let mut indegree = BTreeMap::new();
    let mut users: BTreeMap<NodeId, BTreeSet<NodeId>> = BTreeMap::new();
    for (id, d) in &records {
        let mut dependencies = BTreeSet::new();
        for source in d.dependencies() {
            if let Some(node) = doc.node(source) {
                node.walk(&mut |n| {
                    if records.contains_key(&n.id) {
                        dependencies.insert(n.id);
                    }
                });
            }
            if records.contains_key(&source) {
                dependencies.insert(source);
            }
            if let Some(ancestors) = doc.ancestry(source) {
                for parent in ancestors {
                    if records.contains_key(&parent) {
                        dependencies.insert(parent);
                    }
                }
            }
        }
        indegree.insert(*id, dependencies.len());
        for dependency in dependencies {
            users.entry(dependency).or_default().insert(*id);
        }
    }
    let mut ready: VecDeque<_> = indegree.iter().filter_map(|(id, n)| (*n == 0).then_some(*id)).collect();
    let mut done = BTreeSet::new();
    while let Some(id) = ready.pop_front() {
        let Some(mut spec) = records.get(&id).cloned() else { continue };
        let unchanged = before.node(id).is_some_and(|old| {
            old.extras().nurbs_debris.as_deref() == Some(&spec)
                && doc.node(id).is_some_and(|n| n.kind == old.kind)
                && spec.dependencies().into_iter().all(|source| before.node(source) == doc.node(source))
        });
        if !unchanged {
            let built = spec.build(doc);
            let children = match built {
                Ok(mut n) => {
                    let outline = (spec.display == DisplayMode::Outline).then(|| outline_art(&n));
                    n.visible = spec.display == DisplayMode::Normal;
                    allocate(&mut n, doc);
                    let mut children = vec![Arc::new(n)];
                    if let Some(mut outline) = outline {
                        allocate(&mut outline, doc);
                        children.push(Arc::new(outline));
                    }
                    spec.error = None;
                    children
                }
                Err(e) => {
                    spec.error = Some(e);
                    vec![]
                }
            };
            if let Some(n) = doc.node_mut(id) {
                n.kind = NodeKind::Group { children, clip: false };
                n.extras_mut().nurbs_debris = Some(Box::new(spec));
            }
        }
        done.insert(id);
        for user in users.get(&id).into_iter().flatten() {
            if let Some(n) = indegree.get_mut(user) {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    ready.push_back(*user);
                }
            }
        }
    }
    for (id, mut spec) in records {
        if !done.contains(&id) {
            spec.error = Some("debris references form a cycle".into());
            if let Some(n) = doc.node_mut(id) {
                n.kind = NodeKind::Group { children: vec![], clip: false };
                n.extras_mut().nurbs_debris = Some(Box::new(spec));
            }
        }
    }
}
pub fn empty_broken(doc: &mut Document) {
    let before = doc.clone();
    let mut ids = vec![];
    doc.walk(|n| {
        if n.extras().nurbs_debris.is_some() {
            ids.push(n.id);
        }
    });
    for id in ids {
        if let Some(n) = doc.node_mut(id)
            && let Some(s) = n.extras_mut().nurbs_debris.as_mut()
        {
            s.error = Some("rebuild after native load".into());
        }
    }
    refresh(&before, doc);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Appearance, ImageObject};
    use vectorcraft_geom::{Rect, SubPath, nurbs::ControlPoint, shapes};
    fn curve(doc: &mut Document, points: Vec<Point>, periodic: bool) -> NodeId {
        let c = if periodic {
            NurbsPath::with_topology(points.into_iter().map(|p| ControlPoint::new(p, 1.0)).collect(), vec![], true).unwrap()
        } else {
            NurbsPath::from_controls(points.into_iter().map(|p| ControlPoint::new(p, 1.0)).collect()).unwrap()
        };
        let preview = PathData::from_bezpath(&c.to_bezpath(0.01).unwrap());
        let id = doc.alloc_id();
        let mut n = Node::path(id, preview.clone(), Appearance::default_art());
        n.kind = NodeKind::Path {
            path: preview.clone(),
            live: Some(Box::new(LiveShape::Nurbs { curve: c, xf: Affine::IDENTITY, preview, color_stroke: None })),
            rule: Default::default(),
            clipping: false,
            guide: false,
        };
        doc.insert(None, usize::MAX, n).unwrap();
        id
    }
    fn setup() -> (Document, NodeId, NodeId) {
        let mut doc = Document::new(200.0, 200.0);
        let base = curve(&mut doc, vec![Point::ZERO, Point::new(100.0, 0.0)], false);
        let id = doc.alloc_id();
        doc.insert(None, usize::MAX, Node::path(id, shapes::rectangle(Rect::new(20.0, 10.0, 40.0, 30.0)), Appearance::default_art())).unwrap();
        (doc, base, id)
    }
    fn points(n: &Node) -> Vec<Point> {
        Cloud::of(n).unwrap().points.into_iter().map(|p| p.p).collect()
    }
    #[test]
    fn nurbs_debris_exact_projection_and_bidirectional_links() {
        let (mut doc, base, source) = setup();
        let mut d = Debris::new(&doc, base, source).unwrap();
        let original = points(doc.node(source).unwrap());
        for (a, b) in points(&d.build(&doc).unwrap()).iter().zip(&original) {
            assert!(a.distance(*b) < 1e-8);
        }
        doc.node_mut(base).unwrap().transform(Affine::rotate(std::f64::consts::FRAC_PI_2), false);
        let rotated = points(&d.build(&doc).unwrap());
        for (a, b) in rotated.iter().zip(&original) {
            assert!(a.distance(Point::new(-b.y, b.x)) < 1e-7);
        }
        doc.node_mut(source).unwrap().transform(Affine::translate((5.0, 3.0)), false);
        let updated = points(&d.build(&doc).unwrap());
        for (a, b) in updated.iter().zip(&original) {
            assert!(a.distance(Point::new(-b.y - 3.0, b.x + 5.0)) < 1e-7);
        }
        assert_eq!(d.phases.first().unwrap().source, source);
    }
    #[test]
    fn nurbs_debris_range_roundtrip_preserves_local_offsets() {
        let (doc, base, source) = setup();
        let mut d = Debris::new(&doc, base, source).unwrap();
        let original = points(&d.build(&doc).unwrap());
        let range = d.range().unwrap();
        let anchors = d.phases[0].anchors.clone();
        d.set_range([0.6, 0.9]).unwrap();
        assert_ne!(points(&d.build(&doc).unwrap()), original);
        d.set_range(range).unwrap();
        for (a, b) in points(&d.build(&doc).unwrap()).iter().zip(original) {
            assert!(a.distance(b) < 1e-8);
        }
        assert_eq!(d.phases[0].anchors, anchors);
        d.scale = 2.0;
        d.offset = 5.0;
        let p = points(&d.build(&doc).unwrap());
        assert!((p[0].y - 25.0).abs() < 1e-8);
    }
    #[test]
    fn nurbs_debris_phase_blend_uses_homogeneous_positions_and_short_hsv() {
        let (mut doc, base, _) = setup();
        let a = curve(&mut doc, vec![Point::new(20.0, 10.0), Point::new(40.0, 10.0)], false);
        let b = curve(&mut doc, vec![Point::new(20.0, 30.0), Point::new(40.0, 30.0)], false);
        for (id, w, color) in [(a, 1.0, [1.0, 0.0, 0.0, 1.0]), (b, 3.0, [0.0, 0.0, 1.0, 1.0])] {
            let node = doc.node_mut(id).unwrap();
            let NodeKind::Path { live: Some(l), .. } = &mut node.kind else { panic!() };
            let LiveShape::Nurbs { curve, .. } = &mut **l else { panic!() };
            for i in 0..2 {
                let mut p = curve.controls()[i];
                p.weight = w;
                p.color = color;
                *curve = curve.set_control(i, p).unwrap();
            }
        }
        let mut d = Debris::new(&doc, base, a).unwrap();
        d.add_phase(&doc, b, 1.0).unwrap();
        d.blend = 0.5;
        let n = d.build(&doc).unwrap();
        let c = Cloud::of(&n).unwrap();
        assert!((c.points[0].p.y - 25.0).abs() < 1e-8);
        assert!((c.points[0].weight - 2.0).abs() < 1e-12);
        assert!(c.points[0].color[0] > 0.99 && c.points[0].color[2] > 0.99 && c.points[0].color[1] < 0.01);
        d.blend = 0.0;
        assert!((points(&d.build(&doc).unwrap())[0].y - 10.0).abs() < 1e-8);
        d.blend = 2.0;
        assert!((points(&d.build(&doc).unwrap())[0].y - 30.0).abs() < 1e-8);
    }
    #[test]
    fn nurbs_debris_phase_topology_and_single_phase_replacement() {
        let (mut doc, base, source) = setup();
        let b = curve(&mut doc, vec![Point::new(20.0, 10.0), Point::new(40.0, 30.0)], false);
        let mut d = Debris::new(&doc, base, source).unwrap();
        assert!(d.add_phase(&doc, b, 1.0).is_err());
        assert!(d.add_phase(&doc, b, 0.0).is_ok());
        assert_eq!(d.phases.len(), 1);
        assert!(d.build(&doc).is_ok());
    }
    #[test]
    fn nurbs_debris_periodic_binding_uses_smallest_unwrapped_interval() {
        let mut doc = Document::new(200.0, 200.0);
        let base = curve(&mut doc, vec![Point::new(100.0, 0.0), Point::new(0.0, 100.0), Point::new(-100.0, 0.0), Point::new(0.0, -100.0)], true);
        let c = base_curve(&doc, base).unwrap();
        let ps = [c.evaluate(0.98).unwrap(), c.evaluate(0.02).unwrap()];
        let source = doc.alloc_id();
        doc.insert(None, usize::MAX, Node::path(source, PathData::single(SubPath::polyline(&ps, false)), Appearance::default_art())).unwrap();
        let mut d = Debris::new(&doc, base, source).unwrap();
        let r = d.range().unwrap();
        assert!((r[1] - r[0]).abs() < 0.1, "{r:?}");
        let before = points(doc.node(source).unwrap());
        for (a, b) in points(&d.build(&doc).unwrap()).iter().zip(before) {
            assert!(a.distance(b) < 1e-7);
        }
    }
    #[test]
    fn nurbs_debris_raster_has_four_vertices_and_live_quad_envelope() {
        let (mut doc, base, _) = setup();
        let id = doc.alloc_id();
        let mut n = Node::path(id, PathData::default(), Appearance::default());
        n.kind = NodeKind::Image(Box::new(ImageObject {
            key: "test".into(),
            width: 20,
            height: 10,
            xf: Affine::translate((20.0, 5.0)),
            link: None,
            placement: Default::default(),
            pixels: None,
        }));
        n.visible = false;
        doc.insert(None, usize::MAX, n).unwrap();
        let mut d = Debris::new(&doc, base, id).unwrap();
        assert_eq!(Cloud::of(doc.node(id).unwrap()).unwrap().vertices, 4);
        d.scale = 2.0;
        let built = d.build(&doc).unwrap();
        let NodeKind::Envelope { kind, content, .. } = built.kind else { panic!("live image quad") };
        let crate::live::EnvelopeKind::Mesh { points, .. } = *kind else { panic!() };
        assert!(points[0].distance(Point::new(20.0, 10.0)) < 1e-8, "quad: {points:?}");
        assert!(points[3].distance(Point::new(40.0, 30.0)) < 1e-8);
        assert!(content[0].visible);
    }
    #[test]
    fn nurbs_debris_cycles_empty_groups_instead_of_recursing() {
        let (mut doc, base, source) = setup();
        let mut d = Debris::new(&doc, base, source).unwrap();
        let id = doc.alloc_id();
        d.phases[0].source = id;
        let mut n = Node::group(id, vec![]);
        n.extras_mut().nurbs_debris = Some(Box::new(d));
        let before = doc.clone();
        doc.insert(None, usize::MAX, n).unwrap();
        refresh(&before, &mut doc);
        let n = doc.node(id).unwrap();
        assert!(n.children().unwrap().is_empty());
        assert!(n.extras().nurbs_debris.as_ref().unwrap().error.as_ref().unwrap().contains("cycle"));
    }
}
