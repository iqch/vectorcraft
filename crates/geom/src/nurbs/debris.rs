//! Parameter-space attachment of point clouds to composed NURBS paths.
use super::{NurbsError, NurbsPath};
use kurbo::{Point, Vec2};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DebrisAnchor {
    pub u: f64,
    pub tangent: f64,
    pub normal: f64,
}
pub struct DebrisFrames<'a> {
    curve: &'a NurbsPath,
    grid: Vec<(f64, Point, f64)>,
    length: f64,
}
impl<'a> DebrisFrames<'a> {
    pub fn new(curve: &'a NurbsPath) -> Result<Self, NurbsError> {
        let mut params = curve.knots();
        params.extend((0..=512).map(|i| i as f64 / 512.0));
        if params.len() > 16384 {
            return Err(NurbsError::Size);
        }
        params.sort_by(f64::total_cmp);
        params.dedup();
        let mut grid = Vec::with_capacity(params.len());
        let mut length = 0.0;
        for u in params {
            let p = curve.evaluate_composed(u)?;
            if let Some((_, last, _)) = grid.last() {
                length += p.distance(*last);
            }
            grid.push((u, p, length));
        }
        if !length.is_finite() || length <= 1e-12 {
            return Err(NurbsError::Numerical);
        }
        Ok(Self { curve, grid, length })
    }
    pub fn length(&self) -> f64 {
        self.length
    }
    pub fn frame(&self, u: f64) -> Result<(Point, Vec2, Vec2), NurbsError> {
        if !u.is_finite() {
            return Err(NurbsError::Parameter);
        }
        let param = |u: f64| if self.curve.is_periodic() { u.rem_euclid(1.0) } else { u.clamp(0.0, 1.0) };
        let u = param(u);
        let p = self.curve.evaluate_composed(u)?;
        for h in [1e-6, 1e-4, 1e-2] {
            let d = self.curve.evaluate_composed(param(u + h))? - self.curve.evaluate_composed(param(u - h))?;
            let n = d.hypot();
            if n.is_finite() && n > 1e-12 {
                let t = d / n;
                return Ok((p, t, Vec2::new(-t.y, t.x)));
            }
        }
        Err(NurbsError::Numerical)
    }
    pub fn nearest(&self, p: Point) -> Result<f64, NurbsError> {
        if !p.x.is_finite() || !p.y.is_finite() {
            return Err(NurbsError::Control);
        }
        let mut best = (f64::INFINITY, 0usize);
        for (i, w) in self.grid.windows(2).enumerate() {
            let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
            let d = b.1 - a.1;
            let n = d.hypot2();
            let t = if n > 0.0 { ((p - a.1).dot(d) / n).clamp(0.0, 1.0) } else { 0.0 };
            let distance = (p - (a.1 + d * t)).hypot2();
            if distance < best.0 {
                best = (distance, i);
            }
        }
        let k = best.1;
        let mut a = self.grid.get(k.saturating_sub(1)).ok_or(NurbsError::Numerical)?.0;
        let mut b = self.grid.get((k + 2).min(self.grid.len().saturating_sub(1))).ok_or(NurbsError::Numerical)?.0;
        for _ in 0..56 {
            let x = a + (b - a) / 3.0;
            let y = b - (b - a) / 3.0;
            if self.curve.evaluate_composed(x)?.distance_squared(p) <= self.curve.evaluate_composed(y)?.distance_squared(p) {
                b = y;
            } else {
                a = x;
            }
        }
        let mut u = (a + b) * 0.5;
        for candidate in [0.0, 1.0, self.grid.get(k).ok_or(NurbsError::Numerical)?.0, self.grid.get(k + 1).ok_or(NurbsError::Numerical)?.0] {
            if self.curve.evaluate_composed(candidate)?.distance_squared(p) < self.curve.evaluate_composed(u)?.distance_squared(p) {
                u = candidate;
            }
        }
        // Squared distance loses the tangential residual when the normal offset is large.
        // Polish the stationary point before that rounding plateau affects scaled offsets.
        for _ in 0..4 {
            let h = 1e-5_f64.min(u.min(1.0 - u) * 0.25);
            if h <= 1e-12 {
                break;
            }
            let c = self.curve.evaluate_composed(u)?;
            let lo = self.curve.evaluate_composed(u - h)?;
            let hi = self.curve.evaluate_composed(u + h)?;
            let d = (hi - lo) / (2.0 * h);
            let dd = ((hi - c) + (lo - c)) / (h * h);
            let denominator = d.hypot2() + (c - p).dot(dd);
            if !denominator.is_finite() || denominator <= 1e-20 {
                break;
            }
            let step = (c - p).dot(d) / denominator;
            if !step.is_finite() || step.abs() > 1e-5 {
                break;
            }
            let next = (u - step).clamp(0.0, 1.0);
            let distance = c.distance_squared(p);
            if self.curve.evaluate_composed(next)?.distance_squared(p) > distance + 32.0 * f64::EPSILON * (1.0 + distance) {
                break;
            }
            u = next;
            if step.abs() < 1e-14 {
                break;
            }
        }
        Ok(u)
    }
    pub fn project(&self, p: Point, around: f64, inverse_scale: f64, inverse_offset: f64) -> Result<DebrisAnchor, NurbsError> {
        if !around.is_finite() || !inverse_scale.is_finite() || !inverse_offset.is_finite() {
            return Err(NurbsError::Numerical);
        }
        let mut u = self.nearest(p)?;
        let (base, t, n) = self.frame(u)?;
        let d = p - base;
        if self.curve.is_periodic() {
            u += (around - u).round();
        }
        let (tangent, normal) =
            if inverse_scale.abs() > 1e-12 { (d.dot(t) / inverse_scale, (d.dot(n) - inverse_offset) / inverse_scale) } else { (0.0, 0.0) };
        Ok(DebrisAnchor { u, tangent, normal })
    }
    pub fn materialize(&self, a: DebrisAnchor, map: [f64; 2], scale: f64, offset: f64) -> Result<Point, NurbsError> {
        if ![a.u, a.tangent, a.normal, map[0], map[1], scale, offset].iter().all(|v| v.is_finite()) {
            return Err(NurbsError::Numerical);
        }
        let (p, t, n) = self.frame(map[0] + map[1] * a.u)?;
        let p = p + t * (a.tangent * scale) + n * (a.normal * scale + offset);
        if p.x.is_finite() && p.y.is_finite() { Ok(p) } else { Err(NurbsError::Numerical) }
    }
    pub fn length_at(&self, u: f64) -> Result<f64, NurbsError> {
        if !u.is_finite() {
            return Err(NurbsError::Parameter);
        }
        let turns = if self.curve.is_periodic() { u.floor() } else { 0.0 };
        let u = if self.curve.is_periodic() { u - turns } else { u.clamp(0.0, 1.0) };
        let k = self.grid.partition_point(|v| v.0 <= u).clamp(1, self.grid.len() - 1);
        let a = self.grid.get(k - 1).ok_or(NurbsError::Numerical)?;
        let b = self.grid.get(k).ok_or(NurbsError::Numerical)?;
        Ok(turns * self.length + a.2 + (b.2 - a.2) * (u - a.0) / (b.0 - a.0))
    }
    pub fn parameter_at_length(&self, length: f64) -> Result<f64, NurbsError> {
        if !length.is_finite() {
            return Err(NurbsError::Parameter);
        }
        let turns = if self.curve.is_periodic() { (length / self.length).floor() } else { 0.0 };
        let l = if self.curve.is_periodic() { length - turns * self.length } else { length.clamp(0.0, self.length) };
        let k = self.grid.partition_point(|v| v.2 <= l).clamp(1, self.grid.len() - 1);
        let a = self.grid.get(k - 1).ok_or(NurbsError::Numerical)?;
        let b = self.grid.get(k).ok_or(NurbsError::Numerical)?;
        Ok(turns + a.0 + (b.0 - a.0) * if b.2 > a.2 { (l - a.2) / (b.2 - a.2) } else { 0.0 })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::ControlPoint;
    fn line(a: Point, b: Point) -> NurbsPath {
        NurbsPath::from_controls(vec![ControlPoint::new(a, 1.0), ControlPoint::new(b, 1.0)]).unwrap()
    }
    #[test]
    fn nurbs_debris_projection_roundtrip_and_frame_deformation() {
        let a = line(Point::ZERO, Point::new(100.0, 0.0));
        let a = DebrisFrames::new(&a).unwrap();
        let p = Point::new(30.0, 15.0);
        let anchor = a.project(p, 0.5, 1.0, 0.0).unwrap();
        assert!(a.materialize(anchor, [0.0, 1.0], 1.0, 0.0).unwrap().distance(p) < 1e-8);
        let b = line(Point::new(10.0, 20.0), Point::new(10.0, 220.0));
        let b = DebrisFrames::new(&b).unwrap();
        assert!(b.materialize(anchor, [0.0, 1.0], 1.0, 0.0).unwrap().distance(Point::new(-5.0, 80.0)) < 1e-8);
        assert!(a.materialize(anchor, [0.2, 1.0], 2.0, 5.0).unwrap().distance(Point::new(50.0, 35.0)) < 1e-8);
        assert!((a.parameter_at_length(25.0).unwrap() - 0.25).abs() < 1e-12);
    }
}
