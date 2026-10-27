//! Pixel regressions for the standard-paint NURBS region mask.
use crate::{RenderOptions, Renderer};
use vectorcraft_color::{Color, Gradient, GradientKind, GradientPaint, GradientStop, Paint};
use vectorcraft_doc::{
    Appearance, Document, LiveShape, Node, NodeId, NodeKind,
    nurbs_region::{Region, refresh},
};
use vectorcraft_geom::{
    Affine, PathData, Point,
    nurbs::{ControlPoint, NurbsPath},
};
fn curve(doc: &mut Document, x: f64, y: f64, size: f64) -> NodeId {
    let curve = NurbsPath::with_topology(
        vec![
            ControlPoint::new(Point::new(x, y), 1.0),
            ControlPoint::new(Point::new(x + size, y), 1.0),
            ControlPoint::new(Point::new(x + size, y + size), 1.0),
            ControlPoint::new(Point::new(x, y + size), 1.0),
        ],
        vec![0, 1, 2, 3],
        true,
    )
    .unwrap();
    let preview = PathData::from_bezpath(&curve.to_bezpath(0.01).unwrap());
    let id = doc.alloc_id();
    let mut n = Node::path(id, preview.clone(), Appearance::default());
    n.kind = NodeKind::Path {
        path: preview.clone(),
        rule: Default::default(),
        live: Some(Box::new(LiveShape::Nurbs { curve, color_stroke: None, preview, xf: Affine::IDENTITY })),
        clipping: false,
        guide: false,
    };
    doc.insert(None, usize::MAX, n).unwrap();
    id
}
fn document(gradient: bool) -> (Document, NodeId, NodeId) {
    let mut doc = Document::new(120.0, 120.0);
    let outer = curve(&mut doc, 10.0, 10.0, 100.0);
    let inner = curve(&mut doc, 40.0, 40.0, 40.0);
    let id = doc.alloc_id();
    let paint = if gradient {
        Paint::Gradient(Box::new(GradientPaint::new(Gradient {
            kind: GradientKind::Linear,
            stops: vec![GradientStop::new(0.0, Color::rgb(1.0, 0.0, 0.0)), GradientStop::new(1.0, Color::rgb(0.0, 0.0, 1.0))],
            ..Default::default()
        })))
    } else {
        Paint::solid(Color::rgb(1.0, 0.0, 0.0))
    };
    let mut n = Node::path(id, PathData::default(), Appearance::basic(paint, Paint::solid(Color::rgb(0.0, 1.0, 0.0)), 20.0));
    let mut region = Region::new(outer, inner);
    region.profile.points = vec![[0.0, 0.0], [1.0, 1.0]];
    n.extras_mut().nurbs_region = Some(Box::new(region));
    doc.insert(None, usize::MAX, n).unwrap();
    let before = doc.clone();
    // A new relation needs initial materialization even if callers supplied the spec already.
    let mut before = before;
    before.node_mut(id).unwrap().extras_mut().nurbs_region = None;
    refresh(&before, &mut doc).unwrap();
    (doc, id, inner)
}
#[test]
fn nurbs_region_render_multiplies_fill_and_object_alpha_and_has_no_stroke() {
    let (mut doc, id, _) = document(false);
    let before = doc.clone();
    let n = doc.node_mut(id).unwrap();
    n.opacity = 0.5;
    n.appearance.fill_mut().unwrap().opacity = 0.5;
    refresh(&before, &mut doc).unwrap();
    let p = Renderer::new().render(&doc, 120, 120, Affine::IDENTITY, &RenderOptions { background: None, artboards: false, ..Default::default() });
    assert!(p.pixel(60, 60)[3].abs_diff(64) <= 2, "inner full fill: {:?}", p.pixel(60, 60));
    assert!(p.pixel(20, 60)[3].abs_diff(42) <= 5, "ring distance mask: {:?}", p.pixel(20, 60));
    assert_eq!(p.pixel(5, 60)[3], 0, "no outer stroke or fill");
    assert_eq!(p.pixel(115, 60)[3], 0);
}
#[test]
fn nurbs_region_render_preserves_standard_gradient_and_empty_broken_sources() {
    let (mut doc, id, inner) = document(true);
    let mut renderer = Renderer::new();
    let options = RenderOptions { background: None, artboards: false, ..Default::default() };
    let p = renderer.render(&doc, 120, 120, Affine::IDENTITY, &options);
    let (left, right) = (p.pixel(20, 60), p.pixel(99, 60));
    assert!(left[0] > left[2] && right[2] > right[0], "gradient colors {:?}, {:?}", left, right);
    assert!(left[3] > 100 && right[3] > 100);
    let before = doc.clone();
    doc.remove(inner).unwrap();
    refresh(&before, &mut doc).unwrap();
    assert!(doc.node(id).unwrap().extras().mask.is_none());
    let p = renderer.render(&doc, 120, 120, Affine::IDENTITY, &options);
    assert_eq!(p.pixel(60, 60)[3], 0);
    assert_eq!(p.pixel(20, 60)[3], 0);
}

#[test]
fn nurbs_region_render_refreshes_mask_pixels_with_same_image_key() {
    let (mut doc, id, _) = document(false);
    let mut renderer = Renderer::new();
    let options = RenderOptions { background: None, artboards: false, ..Default::default() };
    let original = renderer.render(&doc, 120, 120, Affine::IDENTITY, &options).pixel(20, 60);
    let before = doc.clone();
    doc.node_mut(id).unwrap().extras_mut().nurbs_region.as_mut().unwrap().profile.points = vec![[0.0, 0.25], [1.0, 0.25]];
    refresh(&before, &mut doc).unwrap();
    let p = renderer.render(&doc, 120, 120, Affine::IDENTITY, &options);
    assert!(p.pixel(20, 60)[3].abs_diff(64) <= 2, "new mask: {:?}, old: {:?}", p.pixel(20, 60), original);
    assert!(p.pixel(60, 60)[3] > 250);
}

#[test]
fn nurbs_region_constant_profile_is_fully_opaque_throughout_outer() {
    let (mut doc, id, _) = document(false);
    let before = doc.clone();
    doc.node_mut(id).unwrap().extras_mut().nurbs_region.as_mut().unwrap().profile = vectorcraft_doc::nurbs_color::Profile::constant(1.0);
    refresh(&before, &mut doc).unwrap();
    let p = Renderer::new().render(&doc, 120, 120, Affine::IDENTITY, &RenderOptions { background: None, artboards: false, ..Default::default() });
    assert!(p.pixel(20, 60)[3] > 250);
    assert!(p.pixel(60, 60)[3] > 250);
    assert_eq!(p.pixel(5, 60)[3], 0);
}
