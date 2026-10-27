//! Point extraction and materialization. The phase source keeps the editable art.
use super::*;
use crate::live::EnvelopeKind;
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub p: Point,
    pub weight: f64,
    pub color: [f64; 4],
}
impl Sample {
    fn at(p: Point) -> Self {
        Self { p, weight: 1.0, color: [1.0; 4] }
    }
}
pub struct Cloud {
    pub points: Vec<Sample>,
    pub topology: Vec<String>,
    pub vertices: usize,
}
impl Cloud {
    pub fn of(n: &Node) -> Result<Self, String> {
        let mut c = Self { points: vec![], topology: vec![], vertices: 0 };
        c.collect(n, 0)?;
        if c.points.is_empty() {
            return Err("source has no editable vertices".into());
        }
        Ok(c)
    }
    fn push(&mut self, s: Sample) -> Result<(), String> {
        if self.points.len() >= MAX_POINTS || ![s.p.x, s.p.y, s.weight].iter().all(|v| v.is_finite()) || s.p.x.abs() > 1e10 || s.p.y.abs() > 1e10 {
            return Err("debris source exceeds point or coordinate limits".into());
        }
        self.points.push(s);
        Ok(())
    }
    fn collect(&mut self, n: &Node, depth: usize) -> Result<(), String> {
        if depth > 32 {
            return Err("debris source nesting is too deep".into());
        }
        match &n.kind {
            NodeKind::Path { live: Some(l), .. } if matches!(&**l, LiveShape::Nurbs { .. }) => {
                let LiveShape::Nurbs { curve, xf, .. } = &**l else { return Err("not a NURBS source".into()) };
                self.topology.push(format!(
                    "nurbs:{}:{:?}:{:?}:{}",
                    curve.is_periodic(),
                    curve.corners(),
                    curve.pieces().iter().map(|p| (p.degree(), p.knots())).collect::<Vec<_>>(),
                    serde_json::to_string(curve.pastes()).map_err(|e| e.to_string())?
                ));
                self.vertices += curve.controls().len();
                for c in curve.controls() {
                    self.push(Sample { p: *xf * c.position, weight: c.weight, color: c.color })?;
                }
            }
            NodeKind::Path { path, .. } => {
                self.topology.push(format!("path:{:?}", path.subpaths.iter().map(|s| (s.anchors.len(), s.closed)).collect::<Vec<_>>()));
                for s in &path.subpaths {
                    self.vertices += s.anchors.len();
                    for a in &s.anchors {
                        for p in [a.p, a.h_in, a.h_out] {
                            self.push(Sample::at(p))?;
                        }
                    }
                }
            }
            NodeKind::Group { children, .. } | NodeKind::Compound { children, .. } => {
                let tag = match &n.kind {
                    NodeKind::Group { clip, .. } => format!("group:{clip}:{}", children.len()),
                    _ => format!("compound:{}", children.len()),
                };
                let logical = if n.extras().nurbs_debris.is_some() { children.get(..1).unwrap_or(children) } else { children.as_slice() };
                let tag = if n.extras().nurbs_debris.is_some() { "group:false:1".to_string() } else { tag };
                self.topology.push(tag);
                for c in logical {
                    self.collect(c, depth + 1)?;
                }
            }
            NodeKind::Image(im) => {
                if im.width == 0 || im.height == 0 {
                    return Err("image source is empty".into());
                }
                self.topology.push("image:4".into());
                self.vertices += 4;
                for p in
                    [Point::ZERO, Point::new(im.width as f64, 0.0), Point::new(im.width as f64, im.height as f64), Point::new(0.0, im.height as f64)]
                {
                    self.push(Sample::at(im.xf * p))?;
                }
            }
            NodeKind::Envelope { kind, content, .. } => {
                let EnvelopeKind::Mesh { rows, cols, points, handles } = &**kind else {
                    return Err("expand a procedural warp before using it as debris".into());
                };
                let image_quad = *rows == 1
                    && *cols == 1
                    && handles.is_empty()
                    && content.len() == 1
                    && content.first().is_some_and(|n| matches!(&n.kind, NodeKind::Image(_)));
                if image_quad {
                    self.topology.push("image:4".into());
                    self.vertices += 4;
                    for i in [0, 1, 3, 2] {
                        self.push(Sample::at(*points.get(i).ok_or("invalid image quad")?))?;
                    }
                } else {
                    self.topology.push(format!("envelope:{rows}:{cols}:{}", handles.len()));
                    self.vertices += points.len();
                    for (i, p) in points.iter().enumerate() {
                        self.push(Sample::at(*p))?;
                        if let Some(h) = handles.get(i) {
                            for h in h {
                                self.push(Sample::at(*p + *h))?;
                            }
                        }
                    }
                }
            }
            NodeKind::Mesh(mesh) => {
                self.topology.push(format!("mesh:{}:{}", mesh.rows, mesh.cols));
                self.vertices += mesh.points.len();
                for p in &mesh.points {
                    let rgb = p.color.to_rgb();
                    let c = [rgb[0] as f64, rgb[1] as f64, rgb[2] as f64, p.opacity as f64];
                    self.push(Sample { p: p.p, weight: 1.0, color: c })?;
                    for h in p.handles {
                        self.push(Sample { p: p.p + h, weight: 1.0, color: c })?;
                    }
                }
            }
            _ => return Err(
                "source must be a path, NURBS, point-based group/compound, gradient mesh or image; expand procedural objects or outline text first"
                    .into(),
            ),
        }
        if let Some(mask) = &n.extras().mask {
            self.topology.push("mask".into());
            self.collect(&mask.art, depth + 1)?;
        } else {
            self.topology.push("no-mask".into());
        }
        Ok(())
    }
    pub fn signature(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.topology.hash(&mut h);
        for p in &self.points {
            for x in [p.p.x, p.p.y, p.weight, p.color[0], p.color[1], p.color[2], p.color[3]] {
                x.to_bits().hash(&mut h);
            }
        }
        h.finish()
    }
}
struct Cursor<'a> {
    points: &'a [Sample],
    index: usize,
}
impl Cursor<'_> {
    fn next(&mut self) -> Result<Sample, String> {
        let p = *self.points.get(self.index).ok_or("phase point count changed")?;
        self.index += 1;
        Ok(p)
    }
}
pub fn materialize(source: &Node, points: &[Sample]) -> Result<Node, String> {
    let mut cursor = Cursor { points, index: 0 };
    let mut n = source.clone();
    map_node(&mut n, &mut cursor, 0)?;
    if cursor.index != points.len() {
        return Err("phase topology changed".into());
    }
    n.visible = true;
    n.locked = false;
    Ok(n)
}
fn map_node(n: &mut Node, c: &mut Cursor<'_>, depth: usize) -> Result<(), String> {
    if depth > 32 {
        return Err("debris result nesting is too deep".into());
    }
    if n.extras().nurbs_debris.is_some()
        && let NodeKind::Group { children, .. } = &mut n.kind
    {
        children.truncate(1);
        if let Some(child) = children.first_mut() {
            Arc::make_mut(child).visible = true;
        }
    }
    let original_bounds = n.geometric_bounds();
    let image_template = matches!(&n.kind, NodeKind::Image(_)).then(|| n.clone());
    match &mut n.kind {
        NodeKind::Path { path, live, .. } => {
            if let Some(l) = live.as_mut()
                && let LiveShape::Nurbs { curve, xf, preview, .. } = &mut **l
            {
                let mut out = curve.transformed(*xf).map_err(|e| e.to_string())?;
                for i in 0..out.controls().len() {
                    let p = c.next()?;
                    let mut control = *out.controls().get(i).ok_or("missing NURBS control")?;
                    control.position = p.p;
                    control.weight = p.weight;
                    control.color = p.color;
                    out = out.set_control(i, control).map_err(|e| e.to_string())?;
                }
                *preview = PathData::from_bezpath(&out.to_bezpath(0.05).map_err(|e| e.to_string())?);
                *path = preview.clone();
                *xf = Affine::IDENTITY;
                *curve = out;
            } else {
                *live = None;
                for s in &mut path.subpaths {
                    for a in &mut s.anchors {
                        a.p = c.next()?.p;
                        a.h_in = c.next()?.p;
                        a.h_out = c.next()?.p;
                    }
                }
            }
        }
        NodeKind::Group { children, .. } | NodeKind::Compound { children, .. } => {
            for child in children {
                map_node(Arc::make_mut(child), c, depth + 1)?;
            }
        }
        NodeKind::Image(_) => {
            let quad = [c.next()?.p, c.next()?.p, c.next()?.p, c.next()?.p];
            let mut pixels = image_template.ok_or("image source missing")?;
            let NodeKind::Image(image) = &mut pixels.kind else { return Err("image missing".into()) };
            image.xf = Affine::IDENTITY;
            pixels.visible = true;
            pixels.locked = false;
            pixels.opacity = 1.0;
            pixels.blend = Default::default();
            pixels.extras_mut().mask = None;
            n.kind = NodeKind::Envelope {
                content: vec![Arc::new(pixels)],
                kind: Box::new(EnvelopeKind::Mesh { rows: 1, cols: 1, points: vec![quad[0], quad[1], quad[3], quad[2]], handles: vec![] }),
                fidelity: 25.0,
                editing: false,
                options: Default::default(),
                frame: Box::new(Affine::IDENTITY),
            };
        }
        NodeKind::Envelope { kind, content, .. } => {
            let EnvelopeKind::Mesh { rows, cols, points, handles } = &mut **kind else { return Err("unsupported warp topology".into()) };
            let image_quad = *rows == 1
                && *cols == 1
                && handles.is_empty()
                && content.len() == 1
                && content.first().is_some_and(|n| matches!(&n.kind, NodeKind::Image(_)));
            if image_quad {
                let quad = [c.next()?.p, c.next()?.p, c.next()?.p, c.next()?.p];
                *points = vec![quad[0], quad[1], quad[3], quad[2]];
            } else {
                for (i, p) in points.iter_mut().enumerate() {
                    *p = c.next()?.p;
                    if let Some(h) = handles.get_mut(i) {
                        for h in h {
                            *h = c.next()?.p - *p;
                        }
                    }
                }
            }
        }
        NodeKind::Mesh(mesh) => {
            for p in &mut mesh.points {
                let center = c.next()?;
                p.p = center.p;
                p.color = Color::rgb(center.color[0] as f32, center.color[1] as f32, center.color[2] as f32);
                p.opacity = center.color[3] as f32;
                for h in &mut p.handles {
                    *h = c.next()?.p - p.p;
                }
            }
        }
        _ => return Err("unsupported point topology".into()),
    }
    if let Some(mask) = n.extras_mut().mask.as_mut() {
        map_node(Arc::make_mut(&mut mask.art), c, depth + 1)?;
    }
    {
        let mut e = n.extras_mut();
        e.nurbs_region = None;
        e.nurbs_debris = None;
        e.objects_on_path = None;
        e.dimension = None;
        e.shaper = None;
        e.graph = None;
    }
    if let (Some(from), Some(to)) = (original_bounds, n.geometric_bounds()) {
        n.appearance.rebase_gradients(from, to);
    }
    Ok(())
}
