//! Pixel checks for source-linked debris, including translucent image corners.
use crate::{RenderOptions, Renderer};
use std::io::Cursor;
use vectorcraft_doc::{
    Appearance, Document, ImageBlob, ImageObject, LiveShape, Node, NodeId, NodeKind,
    nurbs_debris::{Debris, refresh},
};
use vectorcraft_geom::{
    Affine, PathData, Point,
    nurbs::{ControlPoint, NurbsPath},
};
fn document(alpha: u8) -> (Document, NodeId, NodeId) {
    let mut doc = Document::new(120.0, 120.0);
    let c = NurbsPath::from_controls(vec![ControlPoint::new(Point::new(0.0, 0.0), 1.0), ControlPoint::new(Point::new(120.0, 0.0), 1.0)]).unwrap();
    let preview = PathData::from_bezpath(&c.to_bezpath(0.01).unwrap());
    let base = doc.alloc_id();
    let mut n = Node::path(base, preview.clone(), Appearance::default());
    n.kind = NodeKind::Path {
        path: preview.clone(),
        live: Some(Box::new(LiveShape::Nurbs { curve: c, xf: Affine::IDENTITY, preview, color_stroke: None })),
        rule: Default::default(),
        clipping: false,
        guide: false,
    };
    doc.insert(None, usize::MAX, n).unwrap();
    let pixels = image::RgbaImage::from_pixel(40, 40, image::Rgba([240, 30, 10, alpha]));
    let mut png = Cursor::new(vec![]);
    image::DynamicImage::ImageRgba8(pixels).write_to(&mut png, image::ImageFormat::Png).unwrap();
    doc.images.insert("debris-image".into(), ImageBlob::new("image/png", png.into_inner()));
    let source = doc.alloc_id();
    let mut n = Node::path(source, PathData::default(), Appearance::default());
    n.kind = NodeKind::Image(Box::new(ImageObject {
        key: "debris-image".into(),
        width: 40,
        height: 40,
        xf: Affine::translate((20.0, 20.0)),
        link: None,
        placement: Default::default(),
        pixels: None,
    }));
    n.visible = false;
    doc.insert(None, usize::MAX, n).unwrap();
    let spec = Debris::new(&doc, base, source).unwrap();
    let before = doc.clone();
    let id = doc.alloc_id();
    let mut n = Node::group(id, vec![]);
    n.extras_mut().nurbs_debris = Some(Box::new(spec));
    doc.insert(None, usize::MAX, n).unwrap();
    refresh(&before, &mut doc);
    (doc, base, id)
}
fn render(doc: &Document) -> crate::Rendered {
    Renderer::new().render(doc, 120, 120, Affine::IDENTITY, &RenderOptions { background: None, artboards: false, ..Default::default() })
}
#[test]
fn nurbs_debris_raster_four_corner_render_tracks_base_and_object_opacity() {
    let (mut doc, base, id) = document(255);
    let before = doc.clone();
    doc.node_mut(id).unwrap().opacity = 0.5;
    doc.node_mut(base).unwrap().transform(Affine::new([1.0, 0.5, 0.0, 1.0, 0.0, 0.0]), false);
    refresh(&before, &mut doc);
    let p = render(&doc);
    assert!(p.pixel(40, 60)[3].abs_diff(128) <= 3, "{:?}", p.pixel(40, 60));
    assert_eq!(p.pixel(100, 60)[3], 0);
    assert_eq!(p.pixel(40, 10)[3], 0);
    let group = doc.node(id).unwrap();
    let NodeKind::Group { children, .. } = &group.kind else { panic!() };
    assert!(matches!(&children[0].kind, NodeKind::Envelope { .. }));
    let mut shifted = doc.clone();
    let before = shifted.clone();
    shifted.node_mut(id).unwrap().extras_mut().nurbs_debris.as_mut().unwrap().offset = 10.0;
    refresh(&before, &mut shifted);
    assert_ne!(render(&shifted).pixel(30, 42), p.pixel(30, 42));
}
#[test]
fn nurbs_debris_translucent_raster_does_not_accumulate_triangle_alpha() {
    let (doc, _, _) = document(128);
    let p = render(&doc);
    for y in 23..57 {
        for x in 23..57 {
            assert!(p.pixel(x, y)[3].abs_diff(128) <= 3, "overlapping warp triangles at ({x},{y}): {:?}", p.pixel(x, y));
        }
    }
}

#[test]
fn nurbs_debris_non_affine_image_quad_has_uniform_translucency() {
    let (mut doc, _, id) = document(128);
    let n = doc.node_mut(id).unwrap();
    let NodeKind::Group { children, .. } = &mut n.kind else { panic!() };
    let n = std::sync::Arc::make_mut(&mut children[0]);
    let NodeKind::Envelope { kind, .. } = &mut n.kind else { panic!() };
    let vectorcraft_doc::live::EnvelopeKind::Mesh { points, .. } = &mut **kind else { panic!() };
    *points = vec![Point::new(20.0, 20.0), Point::new(80.0, 25.0), Point::new(15.0, 80.0), Point::new(65.0, 85.0)];
    let p = render(&doc);
    for y in 35..70 {
        for x in 30..55 {
            assert!(p.pixel(x, y)[3].abs_diff(128) <= 2, "quad seam at ({x},{y}): {:?}", p.pixel(x, y));
        }
    }
    assert_eq!(p.pixel(5, 5)[3], 0);
}

#[test]
fn nurbs_debris_non_affine_raster_refreshes_pixels_without_geometry_change() {
    let (mut doc, _, id) = document(128);
    let NodeKind::Group { children, .. } = &mut doc.node_mut(id).unwrap().kind else { panic!() };
    let NodeKind::Envelope { kind, .. } = &mut std::sync::Arc::make_mut(&mut children[0]).kind else { panic!() };
    let vectorcraft_doc::live::EnvelopeKind::Mesh { points, .. } = &mut **kind else { panic!() };
    *points = vec![Point::new(20.0, 20.0), Point::new(80.0, 25.0), Point::new(15.0, 80.0), Point::new(65.0, 85.0)];
    let mut renderer = Renderer::new();
    let opts = RenderOptions { background: None, artboards: false, ..Default::default() };
    let before = renderer.render(&doc, 120, 120, Affine::IDENTITY, &opts).pixel(40, 40);
    assert!(before[0] > before[1]);
    let pixels = image::RgbaImage::from_pixel(40, 40, image::Rgba([10, 230, 20, 128]));
    let mut png = Cursor::new(vec![]);
    pixels.write_to(&mut png, image::ImageFormat::Png).unwrap();
    doc.images.insert("debris-image".into(), ImageBlob::png(png.into_inner()));
    let after = renderer.render(&doc, 120, 120, Affine::IDENTITY, &opts).pixel(40, 40);
    assert!(after[1] > after[0], "stale raster {before:?} -> {after:?}");
    assert!(after[3].abs_diff(128) <= 2);
}

#[test]
fn nurbs_debris_mute_and_outline_render_without_losing_image_geometry() {
    let (mut doc, _, id) = document(128);
    let original = vectorcraft_doc::nurbs_debris::cloud::Cloud::of(doc.node(id).unwrap()).unwrap().points.iter().map(|p| p.p).collect::<Vec<_>>();
    let before = doc.clone();
    doc.node_mut(id).unwrap().extras_mut().nurbs_debris.as_mut().unwrap().display = vectorcraft_doc::nurbs_debris::DisplayMode::Mute;
    refresh(&before, &mut doc);
    let p = render(&doc);
    assert!(p.pixels.iter().skip(3).step_by(4).all(|a| *a == 0));
    let before = doc.clone();
    doc.node_mut(id).unwrap().extras_mut().nurbs_debris.as_mut().unwrap().display = vectorcraft_doc::nurbs_debris::DisplayMode::Outline;
    refresh(&before, &mut doc);
    let p = render(&doc);
    assert_eq!(p.pixel(40, 40)[3], 0);
    assert!(p.pixel(20, 40)[3] > 20, "image frame should be outlined");
    assert_eq!(
        vectorcraft_doc::nurbs_debris::cloud::Cloud::of(doc.node(id).unwrap()).unwrap().points.iter().map(|p| p.p).collect::<Vec<_>>(),
        original
    );
}
