//! Colored strip geometry: standard caps and joins, retaining cross-width shading.
use super::{Profile, Strip};
use crate::{LineCap, LineJoin, StrokeLayer, live::MeshQuad};
use vectorcraft_color::Color;
use vectorcraft_geom::{BezPath, Point, Vec2};
#[derive(Clone, Copy)]
struct Rib {
    left: Point,
    right: Point,
    color: [f64; 4],
    scale: f64,
}
fn unit(v: Vec2) -> Vec2 {
    let l = v.hypot();
    if l > 1e-12 { v / l } else { Vec2::new(1.0, 0.0) }
}
fn polygon(out: &mut BezPath, p: [Point; 4]) {
    // Match the piecewise-linear quad mapping, including folded quads.
    for mut tri in [[p[0], p[1], p[2]], [p[0], p[2], p[3]]] {
        let area = (tri[1] - tri[0]).cross(tri[2] - tri[0]);
        if area.abs() < 1e-15 {
            continue;
        }
        if area < 0.0 {
            tri.swap(1, 2);
        }
        out.move_to(tri[0]);
        out.line_to(tri[1]);
        out.line_to(tri[2]);
        out.close_path();
    }
}
pub(super) fn build(
    samples: &[(Point, Vec2, [f64; 4])],
    points: &[Point],
    periodic: bool,
    stroke: &StrokeLayer,
    opacity: &Profile,
) -> Result<Strip, String> {
    let n = samples.len();
    let mut ribs = Vec::new();
    for (i, &(center, half, color)) in samples.iter().enumerate() {
        if ribs.len() > 65536 {
            return Err("colored NURBS strip exceeds rib budget".into());
        }
        let p = points.get(i).copied().unwrap_or(center);
        let prev = if i == 0 && periodic {
            points.get(n.saturating_sub(2)).copied().unwrap_or(p)
        } else {
            points.get(i.saturating_sub(1)).copied().unwrap_or(p)
        };
        let next = if i + 1 == n && periodic { points.get(1).copied().unwrap_or(p) } else { points.get(i + 1).copied().unwrap_or(p) };
        let incoming = unit(if i == 0 && !periodic { next - p } else { p - prev });
        let outgoing = unit(if i + 1 == n && !periodic { p - prev } else { next - p });
        let (a, b) = (Vec2::new(incoming.y, -incoming.x), Vec2::new(outgoing.y, -outgoing.x));
        let radius = half.hypot();
        let simple = |v: Vec2| Rib { left: center + v, right: center - v, color, scale: 1.0 };
        if !periodic && (i == 0 || i + 1 == n) {
            let t = if i == 0 { outgoing } else { incoming };
            let normal = if i == 0 { b } else { a };
            let outward = if i == 0 { -t } else { t };
            let endpoint = |theta: f64| {
                let width = radius * theta.cos();
                let c = center + outward * (radius * theta.sin());
                Rib { left: c + normal * width, right: c - normal * width, color, scale: theta.cos() }
            };
            if i == 0 && stroke.cap == LineCap::Round {
                for k in (1..=24).rev() {
                    ribs.push(endpoint(k as f64 / 24.0 * std::f64::consts::FRAC_PI_2));
                }
            }
            let c = if stroke.cap == LineCap::Square { center + outward * radius } else { center };
            ribs.push(Rib { left: c + normal * radius, right: c - normal * radius, color, scale: 1.0 });
            if i + 1 == n && stroke.cap == LineCap::Round {
                for k in 1..=24 {
                    ribs.push(endpoint(k as f64 / 24.0 * std::f64::consts::FRAC_PI_2));
                }
            }
            continue;
        }
        let dot = a.dot(b).clamp(-1.0, 1.0);
        let denom = 1.0 + dot;
        let miter = if denom > 1e-8 { (a + b) * (radius / denom) } else { a * radius };
        let limit = if stroke.miter_limit.is_finite() { stroke.miter_limit.max(1.0) } else { 1.0 };
        let turn = incoming.cross(outgoing);
        if dot > 0.99995 || (stroke.join == LineJoin::Miter && denom > 1e-8 && miter.hypot() <= radius * limit) {
            ribs.push(simple(miter));
            continue;
        }
        // Inner intersection shared by all fan ribs: the fan fills just the outer join.
        let inner_vec = if miter.hypot() > radius * limit { unit(miter) * (radius * limit) } else { miter };
        let left_outer = turn >= 0.0;
        let inner = if left_outer { center - inner_vec } else { center + inner_vec };
        let angle = a.cross(b).atan2(dot);
        let count = if stroke.join == LineJoin::Round { (angle.abs() * 24.0).ceil().clamp(1.0, 76.0) as usize } else { 1 };
        for k in 0..=count {
            // The periodic seam join is painted at u=1 only, avoiding a duplicate alpha layer.
            if periodic && i == 0 && k < count {
                continue;
            }
            let v = if k == 0 {
                a
            } else if k == count {
                b
            } else {
                let theta = angle * k as f64 / count as f64;
                Vec2::new(a.x * theta.cos() - a.y * theta.sin(), a.x * theta.sin() + a.y * theta.cos())
            };
            ribs.push(if left_outer {
                Rib { left: center + v * radius, right: inner, color, scale: 1.0 }
            } else {
                Rib { left: inner, right: center - v * radius, color, scale: 1.0 }
            });
        }
    }
    let mut rows: Vec<f64> = (0..=16).map(|i| i as f64 / 16.0).chain(opacity.points.iter().map(|p| p[0])).collect();
    rows.sort_by(f64::total_cmp);
    rows.dedup_by(|a, b| (*a - *b).abs() < 1e-10);
    if ribs.len() > 65536 || ribs.len().saturating_sub(1).saturating_mul(rows.len().saturating_sub(1)) > 1_048_576 {
        return Err("colored NURBS strip exceeds tessellation budget".into());
    }
    let mut outline = BezPath::new();
    let mut quads = Vec::new();
    for pair in ribs.windows(2) {
        let [a, b] = pair else { continue };
        // Consistent orientation forms a union even at folds and self-crossings.
        polygon(&mut outline, [a.left, b.left, b.right, a.right]);
        let at = |r: &Rib, x: f64| r.left + (r.right - r.left) * x;
        let shade = |r: &Rib, x: f64| {
            let c = r.color;
            (Color::rgb(c[0] as f32, c[1] as f32, c[2] as f32), (c[3] * opacity.at(0.5 + (x - 0.5) * r.scale)) as f32)
        };
        for row in rows.windows(2) {
            let (x, y) = (row[0], row[1]);
            let pts = [at(a, x), at(b, x), at(b, y), at(a, y)];
            let corners = [shade(a, x), shade(b, x), shade(b, y), shade(a, y)];
            quads.push(MeshQuad { pts, color: corners[0].0, opacity: corners[0].1, corners });
        }
    }
    Ok(Strip { outline, quads })
}
