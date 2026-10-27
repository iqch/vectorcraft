//! Linked distance-based fills between two periodic NURBS curves.
//! Source geometry remains in document coordinates. The ordinary path and opacity
//! mask are derived together, so native save, standard paints and exports agree.
use crate::{Appearance, Document, ImageBlob, ImageObject, LiveShape, Node, NodeId, NodeKind, OpacityMask, nurbs_color::Profile};
use serde::{Deserialize, Serialize};
use std::{io::Cursor, sync::Arc};
use vectorcraft_color::{Color, Paint};
use vectorcraft_geom::{Affine, BezPath, PathData, Point, Rect, Shape};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Wire", into = "Wire")]
pub struct Region {
    pub outer: Option<NodeId>,
    pub inner: Option<NodeId>,
    pub profile: Profile,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    outer: Option<NodeId>,
    inner: Option<NodeId>,
    profile: Profile,
}
impl TryFrom<Wire> for Region {
    type Error = String;
    fn try_from(w: Wire) -> Result<Self, String> {
        if !w.profile.valid(0.0, 1.0) {
            return Err("alpha spline needs 2..128 ordered finite points, x/y in [0,1], with x endpoints 0 and 1".into());
        }
        Ok(Self { outer: w.outer, inner: w.inner, profile: w.profile })
    }
}
impl From<Region> for Wire {
    fn from(r: Region) -> Self {
        Self { outer: r.outer, inner: r.inner, profile: r.profile }
    }
}
impl Region {
    pub fn new(outer: NodeId, inner: NodeId) -> Self {
        Self { outer: Some(outer), inner: Some(inner), profile: Profile { points: vec![[0.0, 1.0], [1.0, 0.0]] } }
    }
    /// An invalid relationship is an empty object, not a failed document edit.
    pub fn problem(&self, doc: &Document) -> Option<String> {
        if self.outer.is_some() && self.outer == self.inner {
            return Some("Outer and inner sources must differ".into());
        }
        for (role, id) in [("Outer", self.outer), ("Inner", self.inner)] {
            let Some(id) = id else { return Some(format!("{role} curve is not assigned")) };
            if source(doc, id).is_none() {
                return Some(format!("{role} source {id} is missing or is not a periodic NURBS curve"));
            }
        }
        None
    }
    pub fn outlines(&self, doc: &Document) -> Result<(BezPath, BezPath), String> {
        if let Some(e) = self.problem(doc) {
            return Err(e);
        }
        let path = |id: Option<NodeId>| -> Result<BezPath, String> {
            let (curve, xf) = id.and_then(|id| source(doc, id)).ok_or("missing periodic NURBS source")?;
            let mut bp = curve.to_bezpath(0.025).map_err(|e| e.to_string())?;
            bp.apply_affine(xf);
            Ok(bp)
        };
        Ok((path(self.outer)?, path(self.inner)?))
    }
}
/// Only native periodic topology qualifies; a legacy closed preview does not.
pub fn source(doc: &Document, id: NodeId) -> Option<(&vectorcraft_geom::nurbs::NurbsPath, Affine)> {
    let node = doc.node(id)?;
    let NodeKind::Path { live: Some(live), .. } = &node.kind else { return None };
    let LiveShape::Nurbs { curve, xf, .. } = &**live else { return None };
    (curve.is_periodic() && xf.as_coeffs().iter().all(|v| v.is_finite())).then_some((curve, *xf))
}

#[derive(Clone, Copy)]
struct Edge {
    a: Point,
    b: Point,
}
struct Tree {
    bounds: Rect,
    edges: Vec<Edge>,
    children: Option<Box<[Tree; 2]>>,
}
impl Tree {
    fn build(mut edges: Vec<Edge>) -> Self {
        let bounds = edges.iter().fold(Rect::new(f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY), |b, e| {
            Rect::new(b.x0.min(e.a.x).min(e.b.x), b.y0.min(e.a.y).min(e.b.y), b.x1.max(e.a.x).max(e.b.x), b.y1.max(e.a.y).max(e.b.y))
        });
        if edges.len() <= 8 {
            return Self { bounds, edges, children: None };
        }
        let horizontal = bounds.width() >= bounds.height();
        let center = |e: &Edge| if horizontal { e.a.x + e.b.x } else { e.a.y + e.b.y };
        edges.sort_unstable_by(|a, b| center(a).total_cmp(&center(b)));
        let right = edges.split_off(edges.len() / 2);
        Self { bounds, edges: vec![], children: Some(Box::new([Self::build(edges), Self::build(right)])) }
    }
    fn from_path(path: &BezPath, tolerance: f64) -> Result<Self, String> {
        let mut edges = vec![];
        let mut last = Point::ZERO;
        let mut start = Point::ZERO;
        let mut overflow = false;
        vectorcraft_geom::kurbo::flatten(path.iter(), tolerance, |el| {
            if edges.len() >= 32768 {
                overflow = true;
                return;
            }
            match el {
                vectorcraft_geom::kurbo::PathEl::MoveTo(p) => {
                    last = p;
                    start = p;
                }
                vectorcraft_geom::kurbo::PathEl::LineTo(p) => {
                    if p != last {
                        edges.push(Edge { a: last, b: p });
                    }
                    last = p;
                }
                vectorcraft_geom::kurbo::PathEl::ClosePath => {
                    if last != start {
                        edges.push(Edge { a: last, b: start });
                    }
                    last = start;
                }
                _ => {}
            }
        });
        if overflow || edges.is_empty() || edges.iter().any(|e| !e.a.x.is_finite() || !e.a.y.is_finite() || !e.b.x.is_finite() || !e.b.y.is_finite())
        {
            return Err("empty, non-finite or excessively complex NURBS boundary".into());
        }
        Ok(Self::build(edges))
    }
    fn box_distance(&self, p: Point) -> f64 {
        let x = (self.bounds.x0 - p.x).max(0.0).max(p.x - self.bounds.x1);
        let y = (self.bounds.y0 - p.y).max(0.0).max(p.y - self.bounds.y1);
        x * x + y * y
    }
    fn nearest(&self, p: Point, best: &mut f64) {
        if self.box_distance(p) > *best {
            return;
        }
        if let Some(c) = &self.children {
            let (a, b) = if c[0].box_distance(p) < c[1].box_distance(p) { (&c[0], &c[1]) } else { (&c[1], &c[0]) };
            a.nearest(p, best);
            b.nearest(p, best);
        } else {
            for e in &self.edges {
                let v = e.b - e.a;
                let n = v.hypot2();
                let t = if n > 0.0 { ((p - e.a).dot(v) / n).clamp(0.0, 1.0) } else { 0.0 };
                *best = (*best).min((p - (e.a + v * t)).hypot2());
            }
        }
    }
    fn winding(&self, p: Point) -> i32 {
        if p.y < self.bounds.y0 || p.y >= self.bounds.y1 || p.x > self.bounds.x1 {
            return 0;
        }
        if let Some(c) = &self.children {
            return c[0].winding(p) + c[1].winding(p);
        }
        let mut w = 0;
        for e in &self.edges {
            let up = e.a.y <= p.y && e.b.y > p.y;
            let down = e.b.y <= p.y && e.a.y > p.y;
            if up || down {
                let x = e.a.x + (p.y - e.a.y) * (e.b.x - e.a.x) / (e.b.y - e.a.y);
                if x > p.x {
                    w += if up { 1 } else { -1 };
                }
            }
        }
        w
    }
}
/// Bounded spatial index: distance is evaluated on a flattened curve, never on a
/// control polygon. No assumption of concentric, disjoint or nested boundaries.
pub struct Field {
    outer: Tree,
    inner: Tree,
    profile: Profile,
    constant: Option<f64>,
}
impl Field {
    pub fn new(outer: &BezPath, inner: &BezPath, profile: Profile, tolerance: f64) -> Result<Self, String> {
        if !profile.valid(0.0, 1.0) || !tolerance.is_finite() || tolerance <= 0.0 {
            return Err("invalid distance field parameters".into());
        }
        let constant = profile.points.first().map(|p| p[1]).filter(|v| profile.points.iter().all(|p| p[1] == *v));
        Ok(Self { outer: Tree::from_path(outer, tolerance)?, inner: Tree::from_path(inner, tolerance)?, profile, constant })
    }
    pub fn alpha(&self, p: Point) -> f64 {
        if !p.x.is_finite() || !p.y.is_finite() || self.outer.winding(p) == 0 {
            return 0.0;
        }
        if self.constant == Some(1.0) || self.inner.winding(p) != 0 {
            return 1.0;
        }
        if let Some(value) = self.constant {
            return value;
        }
        let (mut outer, mut inner) = (f64::INFINITY, f64::INFINITY);
        self.outer.nearest(p, &mut outer);
        self.inner.nearest(p, &mut inner);
        let (outer, inner) = (outer.sqrt(), inner.sqrt());
        let total = outer + inner;
        if !total.is_finite() || total <= 1e-12 {
            return 1.0;
        }
        self.profile.at(inner / total).clamp(0.0, 1.0)
    }
}
fn key(id: NodeId) -> String {
    format!("nurbs-region-mask-{}", id.0)
}
fn same_sources(before: &Document, doc: &Document, r: &Region) -> bool {
    [r.outer, r.inner].into_iter().all(|id| id.is_none_or(|id| source(before, id) == source(doc, id)))
}
/// Regenerate after every successful edit, including tool previews. Unchanged
/// geometry/profile reuses the encoded mask. Undo restores both parts atomically.
pub fn refresh(before: &Document, doc: &mut Document) -> Result<(), String> {
    let mut regions = vec![];
    let mut detached = vec![];
    doc.walk(|n| {
        if let Some(r) = &n.extras().nurbs_region {
            if matches!(&n.kind, NodeKind::Path { live: None, .. }) {
                regions.push((n.id, (**r).clone()));
            } else {
                // Expanding/converting an object unlinks the procedural geometry;
                // never overwrite a container and lose its children.
                detached.push(n.id);
            }
        }
    });
    for id in detached {
        if let Some(n) = doc.node_mut(id) {
            n.extras_mut().nurbs_region = None;
        }
    }
    for (id, r) in regions {
        let has_stroke = doc
            .node(id)
            .is_some_and(|n| n.appearance.items.iter().any(|item| matches!(item, crate::AppearanceItem::Stroke(s) if !s.paint.is_none())));
        if has_stroke && let Some(n) = doc.node_mut(id) {
            for item in &mut n.appearance.items {
                if let crate::AppearanceItem::Stroke(stroke) = item {
                    stroke.paint = Paint::None;
                }
            }
        }
        let unchanged = before.node(id).is_some_and(|old| {
            old.extras().nurbs_region.as_deref() == Some(&r)
                && doc.node(id).is_some_and(|n| n.kind == old.kind && n.extras().mask == old.extras().mask)
        }) && before.raster_effects_ppi == doc.raster_effects_ppi
            && same_sources(before, doc, &r);
        if unchanged {
            continue;
        }
        let built = r.outlines(doc).and_then(|(outer, inner)| make_mask(doc, id, &r, outer, inner));
        let (path, mask) = match built {
            Ok(v) => v,
            // Broken references intentionally empty the derived art. Resource/encoding
            // failures must be reported, rather than silently hiding otherwise valid art.
            Err(_) if r.problem(doc).is_some() => {
                doc.images.remove(&key(id));
                (PathData::default(), None)
            }
            Err(e) => return Err(e),
        };
        if let Some(n) = doc.node_mut(id) {
            n.kind = NodeKind::Path { path, rule: Default::default(), live: None, clipping: false, guide: false };
            n.extras_mut().mask = mask;
            // This object's contract has fills only, including when a style adds a stroke.
            for it in &mut n.appearance.items {
                if let crate::AppearanceItem::Stroke(s) = it {
                    s.paint = Paint::None;
                }
            }
        }
    }
    Ok(())
}
fn sample_mask(field: &Field, b: Rect, w: u32, h: u32, threads: u16) -> Result<image::RgbaImage, String> {
    let token = crate::cancel::current_token();
    let cancelled = || token.as_ref().is_some_and(crate::cancel::CancelToken::is_cancelled);
    if cancelled() {
        return Err("NURBS region computation cancelled".into());
    }
    let mut pixels = image::RgbaImage::new(w, h);
    let draw = |row: usize, bytes: &mut [u8]| {
        for (i, pixel) in bytes.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            if i % 64 == 0 && cancelled() {
                return;
            }
            let x = i % w as usize;
            let y = row + i / w as usize;
            let p = Point::new(b.x0 + (x as f64 + 0.5) * b.width() / f64::from(w), b.y0 + (y as f64 + 0.5) * b.height() / f64::from(h));
            let v = (255.0 * field.alpha(p)).round() as u8;
            pixel.copy_from_slice(&[v, v, v, 255]);
        }
    };
    let workers = crate::raster_parallel::workers(w as usize * h as usize, threads).min(h as usize).max(1);
    crate::raster_parallel::for_each_band(pixels.as_mut(), w as usize * 4, workers, draw);
    if cancelled() {
        return Err("NURBS region computation cancelled".into());
    }
    Ok(pixels)
}
fn make_mask(doc: &mut Document, id: NodeId, r: &Region, outer: BezPath, inner: BezPath) -> Result<(PathData, Option<Box<OpacityMask>>), String> {
    let b = outer.bounding_box();
    if !b.width().is_finite() || !b.height().is_finite() || b.width() <= 1e-9 || b.height() <= 1e-9 {
        return Ok((PathData::default(), None));
    }
    let ppi = if doc.raster_effects_ppi.is_finite() { doc.raster_effects_ppi.clamp(36.0, 2400.0) } else { 72.0 };
    let scale = (ppi / 72.0).min(1024.0 / b.width().max(b.height()));
    let w = (b.width() * scale).ceil().clamp(1.0, 1024.0) as u32;
    let h = (b.height() * scale).ceil().clamp(1.0, 1024.0) as u32;
    let field = Field::new(&outer, &inner, r.profile.clone(), (b.width() / f64::from(w)).min(b.height() / f64::from(h)).max(1e-6) * 0.05)?;
    let pixels = sample_mask(&field, b, w, h, crate::raster_parallel::default_threads())?;
    let mut encoded = Cursor::new(vec![]);
    pixels.write_to(&mut encoded, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    let key = key(id);
    doc.images.insert(key.clone(), ImageBlob::png(encoded.into_inner()));
    let mut im = Node::path(doc.alloc_id(), PathData::default(), Appearance::default());
    im.kind = NodeKind::Image(Box::new(ImageObject {
        key,
        width: w,
        height: h,
        xf: Affine::translate((b.x0, b.y0)) * Affine::scale_non_uniform(b.width() / f64::from(w), b.height() / f64::from(h)),
        link: None,
        placement: Default::default(),
        pixels: None,
    }));
    // A vector inner fill enforces the full-opacity inner region right up to its
    // boundary, including discontinuous custom profiles.
    let white = Node::path(doc.alloc_id(), PathData::from_bezpath(&inner), Appearance::basic(Paint::solid(Color::WHITE), Paint::None, 0.0));
    let art = Node::group(doc.alloc_id(), vec![Arc::new(im), Arc::new(white)]);
    Ok((PathData::from_bezpath(&outer), Some(Box::new(OpacityMask::new(art, true)))))
}

#[cfg(test)]
mod nurbs_region_tests {
    use super::*;
    fn rectangle(r: Rect) -> BezPath {
        r.to_path(0.01)
    }
    fn linear() -> Profile {
        Profile { points: vec![[0.0, 0.0], [1.0, 1.0]] }
    }
    #[test]
    fn nurbs_region_distance_ratio_and_hard_domain() {
        let outer = rectangle(Rect::new(0.0, 0.0, 100.0, 100.0));
        let inner = rectangle(Rect::new(30.0, 30.0, 70.0, 70.0));
        let f = Field::new(&outer, &inner, linear(), 0.001).unwrap();
        assert_eq!(f.alpha(Point::new(-1.0, 50.0)), 0.0);
        assert_eq!(f.alpha(Point::new(50.0, 50.0)), 1.0);
        assert!((f.alpha(Point::new(10.0, 50.0)) - 2.0 / 3.0).abs() < 1e-10);
        assert!((f.alpha(Point::new(20.0, 50.0)) - 1.0 / 3.0).abs() < 1e-10);
        assert_eq!(f.alpha(Point::new(f64::NAN, 50.0)), 0.0);
    }
    #[test]
    fn nurbs_region_spline_modulates_ratio_not_inner_fill() {
        let outer = rectangle(Rect::new(0.0, 0.0, 100.0, 100.0));
        let inner = rectangle(Rect::new(30.0, 30.0, 70.0, 70.0));
        let profile = Profile { points: vec![[0.0, 0.25], [1.0, 0.25]] };
        let f = Field::new(&outer, &inner, profile, 0.001).unwrap();
        assert_eq!(f.alpha(Point::new(15.0, 50.0)), 0.25);
        assert_eq!(f.alpha(Point::new(50.0, 50.0)), 1.0);
        assert_eq!(f.alpha(Point::new(110.0, 50.0)), 0.0);
    }
    #[test]
    fn nurbs_region_crossing_and_non_nested_contours_clip_to_outer() {
        let outer = rectangle(Rect::new(0.0, 0.0, 100.0, 100.0));
        let inner = rectangle(Rect::new(50.0, 20.0, 150.0, 80.0));
        let f = Field::new(&outer, &inner, linear(), 0.001).unwrap();
        assert_eq!(f.alpha(Point::new(110.0, 50.0)), 0.0);
        assert_eq!(f.alpha(Point::new(75.0, 50.0)), 1.0);
        assert!((f.alpha(Point::new(25.0, 50.0)) - 0.5).abs() < 1e-10);
        let same = Field::new(&outer, &outer, linear(), 0.001).unwrap();
        assert_eq!(same.alpha(Point::new(50.0, 50.0)), 1.0);
    }
    #[test]
    fn nurbs_region_spatial_index_matches_bruteforce() {
        let outer = vectorcraft_geom::kurbo::Circle::new(Point::new(50.0, 50.0), 45.0).to_path(0.01);
        let tree = Tree::from_path(&outer, 0.005).unwrap();
        let mut flattened = BezPath::new();
        vectorcraft_geom::kurbo::flatten(outer.iter(), 0.005, |el| flattened.push(el));
        for x in 0..20 {
            for y in 0..20 {
                let p = Point::new(x as f64 * 5.1, y as f64 * 5.3);
                assert_eq!(tree.winding(p) != 0, flattened.winding(p) != 0, "point {p:?}");
                let mut distance = f64::INFINITY;
                tree.nearest(p, &mut distance);
                assert!((distance.sqrt() - ((p - Point::new(50.0, 50.0)).hypot() - 45.0).abs()).abs() < 0.02);
            }
        }
    }
    #[test]
    fn nurbs_region_rejects_malformed_splines() {
        for points in
            [json_points(vec![]), json_points(vec![[0.0, 0.0]]), json_points(vec![[0.0, 0.0], [0.0, 1.0]]), json_points(vec![[0.0, 0.0], [1.0, 2.0]])]
        {
            assert!(serde_json::from_value::<Region>(serde_json::json!({"outer":2,"inner":3,"profile":points})).is_err());
        }
        let r = Region::new(NodeId(2), NodeId(3));
        assert_eq!(serde_json::from_value::<Region>(serde_json::to_value(&r).unwrap()).unwrap(), r);
    }
    fn json_points(points: Vec<[f64; 2]>) -> serde_json::Value {
        serde_json::json!({"points":points})
    }
}

/// Native files may contain stale derived paths if their source was lost outside
/// the engine. Never display such cached art after opening the file.
pub fn empty_broken(doc: &mut Document) {
    let mut ids = vec![];
    doc.walk(|n| {
        if n.extras().nurbs_region.as_deref().is_some_and(|r| r.problem(doc).is_some()) {
            ids.push(n.id);
        }
    });
    for id in ids {
        doc.images.remove(&key(id));
        if let Some(n) = doc.node_mut(id) {
            n.kind = NodeKind::Path { path: PathData::default(), rule: Default::default(), live: None, clipping: false, guide: false };
            n.extras_mut().mask = None;
        }
    }
}

#[cfg(test)]
mod nurbs_region_wire_tests {
    use super::*;
    #[test]
    fn nurbs_region_metadata_cannot_replace_container_contents() {
        let child = Node::path(NodeId(2), PathData::default(), Appearance::default());
        let mut group = Node::group(NodeId(1), vec![Arc::new(child)]);
        group.extras_mut().nurbs_region = Some(Box::new(Region::new(NodeId(2), NodeId(3))));
        let read: Node = serde_json::from_value(serde_json::to_value(&group).unwrap()).unwrap();
        assert!(read.extras().nurbs_region.is_none());
        let NodeKind::Group { children, .. } = read.kind else { panic!("container must be preserved") };
        assert_eq!(children.len(), 1);
    }
}

#[cfg(test)]
mod parallel_tests {
    use super::*;
    #[test]
    fn nurbs_region_parallel_mask_is_byte_identical() {
        let outer = vectorcraft_geom::kurbo::Circle::new(Point::new(256.0, 256.0), 240.0).to_path(0.01);
        let inner = vectorcraft_geom::kurbo::Circle::new(Point::new(215.0, 265.0), 90.0).to_path(0.01);
        let field = Field::new(&outer, &inner, Profile { points: vec![[0.0, 1.0], [0.4, 0.8], [1.0, 0.0]] }, 0.025).unwrap();
        let bounds = Rect::new(16.0, 16.0, 496.0, 496.0);
        let serial = sample_mask(&field, bounds, 512, 512, 0).unwrap();
        for threads in [2, 8] {
            assert_eq!(sample_mask(&field, bounds, 512, 512, threads).unwrap().as_raw(), serial.as_raw(), "threads {threads}");
        }
        let constant = Field::new(&outer, &inner, Profile::constant(0.25), 0.025).unwrap();
        assert_eq!(constant.alpha(Point::new(256.0, 256.0)), 1.0);
        assert_eq!(constant.alpha(Point::new(450.0, 256.0)), 0.25);
        assert_eq!(constant.alpha(Point::new(0.0, 0.0)), 0.0);
    }
    #[test]
    fn nurbs_region_parallel_mask_preserves_cancellation() {
        let path = Rect::new(0.0, 0.0, 512.0, 512.0).to_path(0.01);
        let field = Field::new(&path, &path, Profile::constant(1.0), 0.025).unwrap();
        let token = crate::cancel::CancelToken::new();
        crate::cancel::scope(&token, None, || {
            let captured = crate::cancel::current_token().unwrap();
            std::thread::scope(|scope| {
                scope
                    .spawn(move || {
                        assert!(!crate::cancel::is_cancelled());
                        captured.cancel();
                    })
                    .join()
                    .unwrap()
            });
            assert!(sample_mask(&field, Rect::new(0.0, 0.0, 512.0, 512.0), 512, 512, 8).is_err());
        });
        assert!(!crate::cancel::is_cancelled());
    }
}
