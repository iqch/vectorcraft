//! Reproducible NURBS CPU benchmark. Run with cargo run --release -p vectorcraft-testkit --example nurbs-perf.
use std::{hint::black_box, time::Instant};
use vectorcraft_color::{Color, Paint};
use vectorcraft_doc::{
    Appearance, Document, LiveShape, Node, NodeId, NodeKind,
    nurbs_color::ColorStroke,
    nurbs_region::{Region, refresh},
};
use vectorcraft_geom::{
    Affine, PathData, Point,
    nurbs::{ControlPoint, NurbsPath},
};
use vectorcraft_render::{RenderOptions, Renderer};
fn curve_node(doc: &mut Document, controls: Vec<ControlPoint>, periodic: bool, corners: Vec<usize>, appearance: Appearance, color: bool) -> NodeId {
    let curve = NurbsPath::with_topology(controls, corners, periodic).unwrap();
    let preview = PathData::from_bezpath(&curve.to_bezpath(0.025).unwrap());
    let id = doc.alloc_id();
    let mut n = Node::path(id, preview.clone(), appearance);
    n.kind = NodeKind::Path {
        path: preview.clone(),
        live: Some(Box::new(LiveShape::Nurbs {
            curve,
            preview,
            xf: Affine::IDENTITY,
            color_stroke: color.then(|| Box::new(ColorStroke::default())),
        })),
        rule: Default::default(),
        clipping: false,
        guide: false,
    };
    doc.insert(None, usize::MAX, n).unwrap();
    id
}
fn mask_doc() -> (Document, NodeId) {
    let mut doc = Document::new(1100.0, 1100.0);
    let circle = |x: f64, y: f64, r: f64, n: usize| {
        (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / n as f64;
                ControlPoint::new(Point::new(x + r * a.cos(), y + r * a.sin()), 1.0)
            })
            .collect()
    };
    let outer = curve_node(&mut doc, circle(550.0, 550.0, 540.0, 16), true, vec![], Appearance::default(), false);
    let inner = curve_node(&mut doc, circle(610.0, 490.0, 180.0, 10), true, vec![], Appearance::default(), false);
    let before = doc.clone();
    let id = doc.alloc_id();
    let mut n = Node::path(id, PathData::default(), Appearance::basic(Paint::solid(Color::BLACK), Paint::None, 0.0));
    n.extras_mut().nurbs_region = Some(Box::new(Region::new(outer, inner)));
    doc.insert(None, usize::MAX, n).unwrap();
    refresh(&before, &mut doc).unwrap();
    (doc, id)
}
fn masks(threads: u16) -> (f64, Vec<u8>) {
    vectorcraft_doc::raster_parallel::set_default_threads(Some(threads));
    let (mut doc, id) = mask_doc();
    let start = Instant::now();
    for i in 0..5 {
        let before = doc.clone();
        doc.node_mut(id).unwrap().extras_mut().nurbs_region.as_mut().unwrap().profile.points[1][1] = (i + 1) as f64 * 0.001;
        refresh(&before, &mut doc).unwrap();
    }
    let ms = start.elapsed().as_secs_f64() * 1000.0 / 5.0;
    let bytes = doc.images[&format!("nurbs-region-mask-{}", id.0)].bytes.as_ref().clone();
    (ms, bytes)
}
fn stroke_doc() -> Document {
    let mut doc = Document::new(1600.0, 1600.0);
    let controls = [
        (80.0, 80.0, [1.0, 0.0, 0.0, 0.55]),
        (1520.0, 1520.0, [1.0, 0.0, 0.0, 0.55]),
        (80.0, 1520.0, [0.0, 0.0, 1.0, 0.7]),
        (1520.0, 80.0, [0.0, 0.0, 1.0, 0.7]),
    ]
    .into_iter()
    .map(|(x, y, color)| {
        let mut c = ControlPoint::new(Point::new(x, y), 1.0);
        c.color = color;
        c
    })
    .collect();
    let mut app = Appearance::default_art();
    app.fill_mut().unwrap().paint = Paint::None;
    let st = app.stroke_mut().unwrap();
    st.width = 180.0;
    st.paint = Paint::solid(Color::WHITE);
    curve_node(&mut doc, controls, false, vec![1, 2], app, true);
    doc
}
fn strokes(doc: &Document, threads: u16) -> (f64, Vec<u8>) {
    let mut renderer = Renderer::new();
    renderer.threads = threads;
    let opts = RenderOptions { background: None, artboards: false, ..Default::default() };
    let reference = renderer.render(doc, 1600, 1600, Affine::IDENTITY, &opts).pixels;
    let start = Instant::now();
    for _ in 0..5 {
        black_box(renderer.render(doc, 1600, 1600, Affine::IDENTITY, &opts));
    }
    (start.elapsed().as_secs_f64() * 1000.0 / 5.0, reference)
}
fn main() {
    let workers = std::thread::available_parallelism().map(|n| n.get().saturating_sub(1).clamp(1, 8) as u16).unwrap_or(1);
    let (a, pa) = masks(0);
    let (b, pb) = masks(workers);
    assert_eq!(pa, pb, "region masks differ");
    let doc = stroke_doc();
    let (c, pc) = strokes(&doc, 0);
    let (d, pd) = strokes(&doc, workers);
    assert_eq!(pc, pd, "colored stroke pixels differ");
    println!("workers: {workers}");
    println!("region refresh (1024px mask, PNG included): serial {a:.2} ms, parallel {b:.2} ms, speedup {:.2}x", a / b);
    println!("colored stroke (1600px, warm geometry cache): serial {c:.2} ms, parallel {d:.2} ms, speedup {:.2}x", c / d);
    println!("region PNGs and colored stroke RGBA are byte-identical");
}
