//! Colored NURBS strips use the same seam-free quad rasterizer as gradient meshes.
use crate::lru::{self, FrameCache};
use crate::{Frame, Renderer, blend_mode};
use std::{mem::size_of, sync::Arc};
use vectorcraft_color::{BlendMode, Paint};
use vectorcraft_doc::nurbs_color::Strip;
use vectorcraft_doc::{LiveShape, Node, NodeKind, StrokeLayer};
use vectorcraft_geom::{Affine, BezPath, Shape};
use vello_cpu::{RenderContext, peniko};
struct CachedStrip {
    node: Arc<Node>,
    stroke: StrokeLayer,
    strip: Arc<Strip>,
    inputs: Arc<crate::fx::DocInputs>,
}
pub(super) struct StripCache {
    entries: FrameCache<(usize, usize, u64), CachedStrip>,
    #[cfg(test)]
    builds: usize,
}
impl Default for StripCache {
    fn default() -> Self {
        Self {
            entries: FrameCache::new(32 << 20, 256),
            #[cfg(test)]
            builds: 0,
        }
    }
}
impl StripCache {
    pub(super) fn sweep(&mut self, stamp: u64) {
        self.entries.sweep(stamp);
    }
}
impl Renderer {
    pub(crate) fn draw_nurbs_color(&mut self, ctx: &mut RenderContext, f: &Frame, n: &Node, st: &StrokeLayer) -> bool {
        let NodeKind::Path { live: Some(live), .. } = &n.kind else { return false };
        let LiveShape::Nurbs { curve, xf, color_stroke: Some(style), .. } = &**live else { return false };
        if !matches!(st.paint, Paint::Solid { .. } | Paint::Gradient(_)) {
            return false;
        }
        let address = n as *const Node as usize;
        let key = (address, st as *const StrokeLayer as usize, f.px.to_bits());
        let inputs = self.doc_inputs(f.doc);
        let cached = self
            .color_strips
            .entries
            .get(&key, self.stamp)
            .filter(|e| std::ptr::eq(Arc::as_ptr(&e.node), n) && e.stroke == *st && e.inputs.cms == inputs.cms)
            .map(|e| e.strip.clone());
        let strip = if let Some(strip) = cached {
            strip
        } else {
            let b = xf.transform_rect_bbox(curve.to_bezpath(0.1).map(|p| p.bounding_box()).unwrap_or_default());
            let device = (b.width() + b.height()) / f.px.max(1e-6);
            let steps = (device / 2.0).ceil() as usize;
            let Ok(strip) = style.strip(curve, *xf, st, steps.clamp(256, 2048)) else {
                return true;
            };
            let strip = Arc::new(strip);
            #[cfg(test)]
            {
                self.color_strips.builds += 1;
            }
            if let Some(source) = self.geom.get(&address).filter(|e| std::ptr::eq(Arc::as_ptr(&e.node), n)).map(|e| e.node.clone()) {
                let bytes = lru::node_bytes(&source)
                    + size_of::<CachedStrip>()
                    + strip.quads.len() * size_of::<vectorcraft_doc::live::MeshQuad>()
                    + std::mem::size_of_val(strip.outline.elements());
                self.color_strips.entries.insert(
                    key,
                    CachedStrip { node: source, stroke: st.clone(), strip: strip.clone(), inputs },
                    bytes,
                    self.stamp,
                );
            }
            strip
        };
        let layered = st.opacity < 1.0 || st.blend != BlendMode::Normal;
        ctx.set_transform(Affine::IDENTITY);
        if layered {
            ctx.push_layer(None, Some(blend_mode(st.blend)), Some(st.opacity), None, None);
        }
        ctx.set_fill_rule(peniko::Fill::NonZero);
        if !crate::live::draw_colored_quads(
            ctx,
            f,
            &strip.outline,
            &strip.quads,
            crate::live::QuadOptions { alpha: self.alpha, grow: 0.0, strip_pixels: true, threads: self.threads },
        ) {
            // Very large viewports: bounded-memory fallback, one isolated strip with copy blending.
            ctx.set_transform(f.view);
            ctx.push_layer(None, None, None, None, None);
            ctx.push_clip_layer(&strip.outline);
            ctx.set_blend_mode(peniko::BlendMode::new(peniko::Mix::Normal, peniko::Compose::Copy));
            for q in &strip.quads {
                let mut path = BezPath::new();
                path.move_to(q.pts[0]);
                for p in q.pts.iter().skip(1) {
                    path.line_to(*p);
                }
                path.close_path();
                ctx.set_paint(f.ink.color(&q.color, q.opacity * self.alpha));
                ctx.fill_path(&path);
            }
            ctx.set_blend_mode(peniko::BlendMode::default());
            ctx.pop_layer();
            ctx.pop_layer();
        }
        if layered {
            ctx.pop_layer();
        }
        ctx.set_transform(f.view);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RenderOptions, Renderer};
    use vectorcraft_color::{Color, Gradient, GradientKind, GradientPaint, GradientStop};
    use vectorcraft_doc::{
        Appearance, Document, NodeId,
        nurbs_color::{ColorStroke, Profile},
    };
    use vectorcraft_geom::{
        PathData, Point,
        nurbs::{ControlPoint, NurbsPath},
    };
    fn document(gradient: bool) -> Document {
        let mut a = ControlPoint::new(Point::new(20.0, 100.0), 1.0);
        a.color = [1.0, 0.0, 0.0, 1.0];
        let mut b = ControlPoint::new(Point::new(180.0, 100.0), 1.0);
        b.color = [0.0, 0.0, 1.0, 1.0];
        let curve = NurbsPath::from_controls(vec![a, b]).unwrap();
        let preview = PathData::from_bezpath(&curve.to_bezpath(0.1).unwrap());
        let mut appearance = Appearance::default_art();
        appearance.fill_mut().unwrap().paint = Paint::None;
        let st = appearance.stroke_mut().unwrap();
        st.width = 40.0;
        st.paint = if gradient {
            Paint::Gradient(Box::new(GradientPaint::new(Gradient {
                kind: GradientKind::Radial,
                stops: vec![GradientStop::new(0.0, Color::rgb(0.0, 1.0, 0.0)), GradientStop::new(1.0, Color::rgb(1.0, 1.0, 0.0))],
                ..Gradient::default()
            })))
        } else {
            Paint::solid(Color::WHITE)
        };
        let style = ColorStroke {
            offset_amount: 20.0,
            offset: Profile::constant(1.0),
            opacity: Profile { points: vec![[0.0, 0.0], [1.0, 1.0]] },
            ..Default::default()
        };
        let mut n = Node::path(NodeId(2), preview.clone(), appearance);
        n.kind = NodeKind::Path {
            path: preview.clone(),
            live: Some(Box::new(LiveShape::Nurbs { curve, preview, xf: Affine::IDENTITY, color_stroke: Some(Box::new(style)) })),
            rule: Default::default(),
            clipping: false,
            guide: false,
        };
        let mut doc = Document::new(200.0, 200.0);
        doc.insert(None, 0, n).unwrap();
        doc
    }
    #[test]
    fn nurbs_color_render_offsets_alpha_and_ignores_radial_geometry() {
        let mut r = Renderer::new();
        let render = |r: &mut Renderer, doc: &Document| {
            r.render(doc, 200, 200, Affine::IDENTITY, &RenderOptions { background: None, artboards: false, ..Default::default() })
        };
        let d = document(false);
        let pixels = render(&mut r, &d);
        let top = pixels.pixel(100, 65);
        let bottom = pixels.pixel(100, 95);
        assert!(top[3] < 70 && bottom[3] > 200);
        assert!(bottom[0] > 70 && bottom[2] > 70 && bottom[1] < 5);
        assert_eq!(pixels.pixel(100, 115)[3], 0);
        let g = render(&mut r, &document(true));
        let c = g.pixel(100, 95);
        assert!(c[1] > 200 && c[2] < 5, "gradient replaces vertex colors");
    }
    #[test]
    fn nurbs_crossing_uses_section_order_without_color_averaging() {
        let make = |reverse: bool, a: f64, b: f64, object_alpha: f32| {
            let mut doc = document(false);
            let mut controls = vec![
                (30.0, 30.0, [1.0, 0.0, 0.0, a]),
                (170.0, 170.0, [1.0, 0.0, 0.0, a]),
                (30.0, 170.0, [0.0, 0.0, 1.0, b]),
                (170.0, 30.0, [0.0, 0.0, 1.0, b]),
            ]
            .into_iter()
            .map(|(x, y, c)| {
                let mut p = ControlPoint::new(Point::new(x, y), 1.0);
                p.color = c;
                p
            })
            .collect::<Vec<_>>();
            if reverse {
                controls.reverse();
            }
            let curve = NurbsPath::with_topology(controls, vec![1, 2], false).unwrap();
            let preview = PathData::from_bezpath(&curve.to_bezpath(0.1).unwrap());
            let n = doc.node_mut(NodeId(2)).unwrap();
            n.opacity = object_alpha;
            n.appearance.stroke_mut().unwrap().width = 20.0;
            n.kind = NodeKind::Path {
                path: preview.clone(),
                live: Some(Box::new(LiveShape::Nurbs { curve, preview, xf: Affine::IDENTITY, color_stroke: Some(Box::new(ColorStroke::default())) })),
                rule: Default::default(),
                clipping: false,
                guide: false,
            };
            doc
        };
        let mut renderer = Renderer::new();
        let opts = RenderOptions { background: None, artboards: false, ..Default::default() };
        let a = renderer.render(&make(false, 1.0, 1.0, 1.0), 200, 200, Affine::IDENTITY, &opts);
        let b = renderer.render(&make(true, 1.0, 1.0, 1.0), 200, 200, Affine::IDENTITY, &opts);
        let c = a.pixel(100, 100);
        let d = b.pixel(100, 100);
        assert!(c[0] < 5 && c[2] > 250 && c[3] > 250, "last blue section: {c:?}");
        assert!(d[0] > 250 && d[2] < 5 && d[3] > 250, "reversed last red section: {d:?}");
        let mixed = renderer.render(&make(false, 1.0, 0.5, 1.0), 200, 200, Affine::IDENTITY, &opts).pixel(100, 100);
        assert!(
            (i32::from(mixed[0]) - 127).abs() < 4 && (i32::from(mixed[2]) - 128).abs() < 4 && mixed[3] > 250,
            "half-blue over opaque red: {mixed:?}"
        );
        let clear_top = renderer.render(&make(false, 1.0, 0.0, 1.0), 200, 200, Affine::IDENTITY, &opts).pixel(100, 100);
        assert!(clear_top[0] > 250 && clear_top[2] < 5 && clear_top[3] > 250, "transparent upper section must preserve lower section: {clear_top:?}");
        let both = renderer.render(&make(false, 0.5, 0.5, 1.0), 200, 200, Affine::IDENTITY, &opts).pixel(100, 100);
        assert!(
            (i32::from(both[0]) - 85).abs() < 4 && (i32::from(both[2]) - 170).abs() < 4 && (i32::from(both[3]) - 192).abs() < 4,
            "two half-alpha sections: {both:?}"
        );
        let faded = renderer.render(&make(false, 1.0, 0.5, 0.5), 200, 200, Affine::IDENTITY, &opts).pixel(100, 100);
        assert!(
            (i32::from(faded[0]) - 127).abs() < 5 && (i32::from(faded[2]) - 128).abs() < 5 && (i32::from(faded[3]) - 128).abs() < 4,
            "object alpha must apply once: {faded:?}"
        );
    }
    #[test]
    fn nurbs_large_raster_has_no_tile_boundary_seam() {
        let mut doc = document(false);
        let curve =
            NurbsPath::from_controls(vec![ControlPoint::new(Point::new(30.0, 30.0), 1.0), ControlPoint::new(Point::new(1070.0, 1070.0), 1.0)])
                .unwrap();
        let preview = PathData::from_bezpath(&curve.to_bezpath(0.1).unwrap());
        let n = doc.node_mut(NodeId(2)).unwrap();
        let style = ColorStroke::default();
        let mesh = style.strip(&curve, Affine::IDENTITY, n.appearance.stroke_mut().unwrap(), 256).unwrap();
        let bounds = mesh.outline.bounding_box();
        let edge = ((bounds.x0.floor() - 2.0).max(0.0) as u32) + 512;
        n.kind = NodeKind::Path {
            path: preview.clone(),
            live: Some(Box::new(LiveShape::Nurbs { curve, preview, xf: Affine::IDENTITY, color_stroke: Some(Box::new(style)) })),
            rule: Default::default(),
            clipping: false,
            guide: false,
        };
        let image =
            Renderer::new().render(&doc, 1100, 1100, Affine::IDENTITY, &RenderOptions { background: None, artboards: false, ..Default::default() });
        for d in -2i32..=2 {
            let x = (edge as i32 + d) as u32;
            let p = image.pixel(x, x);
            assert!(p.iter().all(|v| *v > 250), "tile seam at {x}: {p:?}");
        }
    }
    #[test]
    fn nurbs_periodic_round_join_does_not_paint_seam_alpha_twice() {
        let mut doc = document(false);
        let controls = [(50.0, 50.0), (150.0, 50.0), (150.0, 150.0)]
            .into_iter()
            .map(|(x, y)| {
                let mut p = ControlPoint::new(Point::new(x, y), 1.0);
                p.color = [1.0, 1.0, 1.0, 0.5];
                p
            })
            .collect();
        let curve = NurbsPath::with_topology(controls, vec![0, 1, 2], true).unwrap();
        let preview = PathData::from_bezpath(&curve.to_bezpath(0.1).unwrap());
        let n = doc.node_mut(NodeId(2)).unwrap();
        let stroke = n.appearance.stroke_mut().unwrap();
        stroke.width = 20.0;
        stroke.join = vectorcraft_doc::LineJoin::Round;
        n.kind = NodeKind::Path {
            path: preview.clone(),
            live: Some(Box::new(LiveShape::Nurbs { curve, preview, xf: Affine::IDENTITY, color_stroke: Some(Box::new(ColorStroke::default())) })),
            rule: Default::default(),
            clipping: false,
            guide: false,
        };
        let image =
            Renderer::new().render(&doc, 200, 200, Affine::IDENTITY, &RenderOptions { background: None, artboards: false, ..Default::default() });
        let pixel = image.pixel(43, 47);
        assert!((115..=140).contains(&pixel[3]), "seam fan must have one half-alpha layer: {pixel:?}");
    }
    #[test]
    fn nurbs_color_parallel_pixels_match_serial_crossings_and_tiles() {
        let mut doc = document(false);
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
        let curve = NurbsPath::with_topology(controls, vec![1, 2], false).unwrap();
        let preview = PathData::from_bezpath(&curve.to_bezpath(0.1).unwrap());
        let n = doc.node_mut(NodeId(2)).unwrap();
        n.opacity = 0.6;
        n.appearance.stroke_mut().unwrap().width = 180.0;
        n.kind = NodeKind::Path {
            path: preview.clone(),
            live: Some(Box::new(LiveShape::Nurbs { curve, preview, xf: Affine::IDENTITY, color_stroke: Some(Box::new(ColorStroke::default())) })),
            rule: Default::default(),
            clipping: false,
            guide: false,
        };
        let opts = RenderOptions { background: None, artboards: false, ..Default::default() };
        let mut serial = Renderer::new();
        serial.threads = 0;
        let baseline = serial.render(&doc, 1600, 1600, Affine::IDENTITY, &opts).pixels;
        let mut parallel = Renderer::new();
        parallel.threads = 8;
        let result = parallel.render(&doc, 1600, 1600, Affine::IDENTITY, &opts).pixels;
        assert_eq!(baseline, result, "parallel shading must retain source-over order, alpha and tile halos");
    }
    #[test]
    fn nurbs_color_strip_cache_reuses_pan_and_invalidates_style_and_zoom() {
        let mut doc = document(false);
        let mut renderer = Renderer::new();
        renderer.threads = 0;
        let opts = RenderOptions { background: None, artboards: false, ..Default::default() };
        renderer.render(&doc, 200, 200, Affine::IDENTITY, &opts);
        assert_eq!(renderer.color_strips.builds, 1);
        renderer.render(&doc, 200, 200, Affine::translate((4.0, 3.0)), &opts);
        assert_eq!(renderer.color_strips.builds, 1, "panning reuses the strip");
        doc.node_mut(NodeId(2)).unwrap().appearance.stroke_mut().unwrap().width = 60.0;
        renderer.render(&doc, 200, 200, Affine::IDENTITY, &opts);
        assert_eq!(renderer.color_strips.builds, 2);
        renderer.render(&doc, 200, 200, Affine::scale(1.2), &opts);
        assert_eq!(renderer.color_strips.builds, 3);
    }
}
