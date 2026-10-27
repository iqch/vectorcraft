//! Editable colored NURBS strokes. Profiles are shape-preserving cubic interpolants.
use crate::{StrokeLayer, live::MeshQuad};
use serde::{Deserialize, Serialize};
use vectorcraft_color::Paint;
use vectorcraft_geom::nurbs::{NurbsPath, mix_color};
use vectorcraft_geom::{Affine, BezPath, Point, Vec2};
mod strip;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    #[default]
    Rgb,
    Hsv,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Domain {
    #[default]
    Parameter,
    Length,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub points: Vec<[f64; 2]>,
}
impl Profile {
    pub fn constant(y: f64) -> Self {
        Self { points: vec![[0.0, y], [1.0, y]] }
    }
    pub(crate) fn valid(&self, low: f64, high: f64) -> bool {
        (2..=128).contains(&self.points.len())
            && self.points.first().is_some_and(|p| p[0] == 0.0)
            && self.points.last().is_some_and(|p| p[0] == 1.0)
            && self.points.iter().all(|p| p[0].is_finite() && p[1].is_finite() && (0.0..=1.0).contains(&p[0]) && (low..=high).contains(&p[1]))
            && self.points.windows(2).all(|p| p[1][0] - p[0][0] >= 1e-6)
    }
    pub fn at(&self, x: f64) -> f64 {
        let x = x.clamp(0.0, 1.0);
        let i = self.points.partition_point(|p| p[0] <= x).saturating_sub(1).min(self.points.len().saturating_sub(2));
        let Some(a) = self.points.get(i) else { return 0.0 };
        let Some(b) = self.points.get(i + 1) else { return a[1] };
        let h = b[0] - a[0];
        if h <= 0.0 {
            return a[1];
        }
        let slope = |k: usize| self.points.get(k).zip(self.points.get(k + 1)).map(|(p, q)| (q[1] - p[1]) / (q[0] - p[0])).unwrap_or(0.0);
        let tangent = |k: usize| {
            if k == 0 {
                return slope(0);
            }
            if k + 1 == self.points.len() {
                return slope(k - 1);
            }
            let (l, r) = (slope(k - 1), slope(k));
            if l * r <= 0.0 {
                0.0
            } else {
                let hl = self.points.get(k - 1).map(|p| self.points[k][0] - p[0]).unwrap_or(h);
                let hr = self.points.get(k + 1).map(|p| p[0] - self.points[k][0]).unwrap_or(h);
                let (w1, w2) = (2.0 * hr + hl, hr + 2.0 * hl);
                (w1 + w2) / (w1 / l + w2 / r)
            }
        };
        let t = (x - a[0]) / h;
        let y = (2.0 * t * t * t - 3.0 * t * t + 1.0) * a[1]
            + (t * t * t - 2.0 * t * t + t) * h * tangent(i)
            + (-2.0 * t * t * t + 3.0 * t * t) * b[1]
            + (t * t * t - t * t) * h * tangent(i + 1);
        y.clamp(a[1].min(b[1]), a[1].max(b[1]))
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "StrokeWire", into = "StrokeWire")]
pub struct ColorStroke {
    pub interpolation: ColorMode,
    pub width_domain: Domain,
    pub offset_domain: Domain,
    pub gradient_domain: Domain,
    pub width: Profile,
    pub offset: Profile,
    pub offset_amount: f64,
    pub opacity: Profile,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct StrokeWire {
    interpolation: ColorMode,
    width_domain: Domain,
    offset_domain: Domain,
    gradient_domain: Domain,
    width: Profile,
    offset: Profile,
    offset_amount: f64,
    opacity: Profile,
}
impl Default for StrokeWire {
    fn default() -> Self {
        Self {
            interpolation: ColorMode::Rgb,
            width_domain: Domain::Parameter,
            offset_domain: Domain::Parameter,
            gradient_domain: Domain::Length,
            width: Profile::constant(1.0),
            offset: Profile::constant(0.0),
            offset_amount: 0.0,
            opacity: Profile::constant(1.0),
        }
    }
}
impl Default for ColorStroke {
    fn default() -> Self {
        StrokeWire::default().into_unchecked()
    }
}
impl StrokeWire {
    fn into_unchecked(self) -> ColorStroke {
        ColorStroke {
            interpolation: self.interpolation,
            width_domain: self.width_domain,
            offset_domain: self.offset_domain,
            gradient_domain: self.gradient_domain,
            width: self.width,
            offset: self.offset,
            offset_amount: self.offset_amount,
            opacity: self.opacity,
        }
    }
}
impl TryFrom<StrokeWire> for ColorStroke {
    type Error = String;
    fn try_from(w: StrokeWire) -> Result<Self, String> {
        if !w.width.valid(0.0, 2.0)
            || !w.offset.valid(-1.0, 1.0)
            || !w.opacity.valid(0.0, 1.0)
            || !w.offset_amount.is_finite()
            || w.offset_amount.abs() > 1e6
        {
            return Err("invalid colored NURBS profiles: ordered x in [0,1], width [0,2], offset [-1,1], opacity [0,1], at most 128 points".into());
        }
        Ok(w.into_unchecked())
    }
}
impl From<ColorStroke> for StrokeWire {
    fn from(s: ColorStroke) -> Self {
        Self {
            interpolation: s.interpolation,
            width_domain: s.width_domain,
            offset_domain: s.offset_domain,
            gradient_domain: s.gradient_domain,
            width: s.width,
            offset: s.offset,
            offset_amount: s.offset_amount,
            opacity: s.opacity,
        }
    }
}
pub struct Strip {
    pub outline: BezPath,
    pub quads: Vec<MeshQuad>,
}
fn coord(d: Domain, u: f64, l: f64) -> f64 {
    if d == Domain::Length { l } else { u }
}
impl ColorStroke {
    pub fn validate(&self) -> Result<(), String> {
        Self::try_from(StrokeWire::from(self.clone())).map(|_| ())
    }
    /// Finite conservative padding from the unchanged NURBS fill boundary.
    pub fn outset(&self, width: f64) -> f64 {
        width.abs() + self.offset_amount.abs()
    }
    fn paint_at(&self, paint: &Paint, vertex: [f64; 4], t: f64) -> [f64; 4] {
        match paint {
            Paint::Solid { color, .. } => {
                let c = color.to_rgb();
                [vertex[0] * f64::from(c[0]), vertex[1] * f64::from(c[1]), vertex[2] * f64::from(c[2]), vertex[3]]
            }
            Paint::Gradient(g) => {
                // Only stop colors, alpha and positions matter; ignore gradient geometry/midpoints.
                let stops = &g.gradient.stops;
                let rgba = |s: &vectorcraft_color::GradientStop| {
                    let c = s.color.to_rgb();
                    [f64::from(c[0]), f64::from(c[1]), f64::from(c[2]), f64::from(s.opacity)]
                };
                let mut before = None;
                let mut after = None;
                for s in stops {
                    let x = f64::from(s.offset);
                    if x <= t && before.is_none_or(|p: &vectorcraft_color::GradientStop| p.offset <= s.offset) {
                        before = Some(s);
                    }
                    if x > t && after.is_none_or(|p: &vectorcraft_color::GradientStop| p.offset > s.offset) {
                        after = Some(s);
                    }
                }
                match (before, after) {
                    (Some(a), Some(b)) => mix_color(
                        rgba(a),
                        rgba(b),
                        ((t - f64::from(a.offset)) / f64::from(b.offset - a.offset)).clamp(0.0, 1.0),
                        self.interpolation == ColorMode::Hsv,
                    ),
                    (Some(a), None) | (None, Some(a)) => rgba(a),
                    _ => [0.0; 4],
                }
            }
            _ => [0.0; 4],
        }
    }
    /// Bounded strip sampling: exact knot/paste boundaries plus a dense parameter grid.
    pub fn strip(&self, curve: &NurbsPath, xf: Affine, stroke: &StrokeLayer, steps: usize) -> Result<Strip, String> {
        self.validate()?;
        let steps = steps.clamp(64, 8192);
        let mut us: Vec<f64> = (0..=steps).map(|i| i as f64 / steps as f64).collect();
        fn breaks(c: &NurbsPath, a: f64, b: f64, out: &mut Vec<f64>) -> Result<(), String> {
            for u in c.knots() {
                out.push(a + (b - a) * u);
            }
            for p in c.pastes().iter().filter(|p| p.enabled) {
                let (r, _) = c.paste_ranges(&p.name).map_err(|e| e.to_string())?;
                for u in r {
                    out.push(a + (b - a) * u.rem_euclid(1.0));
                }
                // Interior samples are prepared by colored_samples; direct interval boundaries must be exact.
            }
            Ok(())
        }
        breaks(curve, 0.0, 1.0, &mut us)?;
        if us.len() > 32769 {
            return Err("too many colored NURBS samples".into());
        }
        us.sort_by(f64::total_cmp);
        us.dedup_by(|a, b| (*a - *b).abs() < 1e-10);
        let values = curve.colored_samples(&us, self.interpolation == ColorMode::Hsv).map_err(|e| e.to_string())?;
        let points: Vec<Point> = values.iter().map(|v| xf * v.0).collect();
        let mut lengths = vec![0.0];
        for p in points.windows(2) {
            lengths.push(lengths.last().copied().unwrap_or(0.0) + p[0].distance(p[1]));
        }
        let total = lengths.last().copied().unwrap_or(0.0).max(1e-12);
        let n = points.len();
        let unit = |v: Vec2| {
            let l = v.hypot();
            if l > 1e-12 { v / l } else { Vec2::new(1.0, 0.0) }
        };
        let mut ribs = Vec::with_capacity(n);
        for (i, &p) in points.iter().enumerate() {
            let prev = if i == 0 && curve.is_periodic() {
                points.get(n.saturating_sub(2)).copied().unwrap_or(p)
            } else {
                points.get(i.saturating_sub(1)).copied().unwrap_or(p)
            };
            let next = if i + 1 == n && curve.is_periodic() { points.get(1).copied().unwrap_or(p) } else { points.get(i + 1).copied().unwrap_or(p) };
            let t = unit(next - prev);
            let normal = Vec2::new(t.y, -t.x);
            let u = us.get(i).copied().unwrap_or(0.0);
            let l = lengths.get(i).copied().unwrap_or(0.0) / total;
            let center = p + normal * (self.offset.at(coord(self.offset_domain, u, l)) * self.offset_amount);
            let half = stroke.width * 0.5 * self.width.at(coord(self.width_domain, u, l));
            let color = self.paint_at(&stroke.paint, values.get(i).map(|v| v.1).unwrap_or([1.0; 4]), coord(self.gradient_domain, u, l));
            ribs.push((center, normal * half, color));
        }
        strip::build(&ribs, &points, curve.is_periodic(), stroke, &self.opacity)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use vectorcraft_color::Color;
    use vectorcraft_geom::Shape;
    use vectorcraft_geom::nurbs::ControlPoint;
    #[test]
    fn nurbs_color_profiles_bounds_and_asymmetric_opacity() {
        let mut s = ColorStroke::default();
        s.width = Profile { points: vec![[0.0, 0.0], [0.5, 2.0], [1.0, 0.0]] };
        s.offset = Profile::constant(-1.0);
        s.offset_amount = 30.0;
        s.opacity = Profile { points: vec![[0.0, 0.0], [1.0, 1.0]] };
        for i in 0..=1000 {
            assert!((0.0..=2.0).contains(&s.width.at(i as f64 / 1000.0)));
        }
        let c =
            NurbsPath::from_controls(vec![ControlPoint::new(Point::new(0.0, 50.0), 1.0), ControlPoint::new(Point::new(100.0, 50.0), 1.0)]).unwrap();
        let st = StrokeLayer::new(Paint::solid(Color::WHITE), 10.0);
        let strip = s.strip(&c, Affine::IDENTITY, &st, 100).unwrap();
        let b = strip.outline.bounding_box();
        assert!((b.y0 - 70.0).abs() < 1e-8 && (b.y1 - 90.0).abs() < 1e-8);
        assert_eq!(strip.quads[0].corners[0].1, 0.0);
        assert_eq!(strip.quads.last().unwrap().corners[2].1, 1.0);
        let wire = serde_json::to_value(&s).unwrap();
        assert_eq!(s, serde_json::from_value::<ColorStroke>(wire.clone()).unwrap());
        let mut invalid = wire;
        invalid["width"]["points"][0][1] = serde_json::json!(3);
        assert!(serde_json::from_value::<ColorStroke>(invalid).is_err());
    }
    #[test]
    fn nurbs_standard_caps_joins_and_miter_limit() {
        use crate::{LineCap, LineJoin};
        let cp = |x, y| ControlPoint::new(Point::new(x, y), 1.0);
        let line = NurbsPath::from_controls(vec![cp(20.0, 50.0), cp(80.0, 50.0)]).unwrap();
        let style = ColorStroke::default();
        let mut st = StrokeLayer::new(Paint::solid(Color::WHITE), 20.0);
        for (cap, left) in [(LineCap::Butt, 20.0), (LineCap::Round, 10.0), (LineCap::Square, 10.0)] {
            st.cap = cap;
            let mesh = style.strip(&line, Affine::IDENTITY, &st, 64).unwrap();
            let b = mesh.outline.bounding_box();
            assert!((b.x0 - left).abs() < 1e-7 && (b.x1 - (100.0 - left)).abs() < 1e-7);
            let inside = |p| mesh.outline.winding(p) != 0;
            assert_eq!(inside(Point::new(11.0, 41.0)), cap == LineCap::Square);
            assert_eq!(inside(Point::new(11.0, 50.0)), cap != LineCap::Butt);
        }
        let corner = NurbsPath::with_topology(vec![cp(20.0, 80.0), cp(50.0, 50.0), cp(80.0, 80.0)], vec![1], false).unwrap();
        st.cap = LineCap::Butt;
        for join in [LineJoin::Miter, LineJoin::Round, LineJoin::Bevel] {
            st.join = join;
            st.miter_limit = 10.0;
            let mesh = style.strip(&corner, Affine::IDENTITY, &st, 64).unwrap();
            let b = mesh.outline.bounding_box();
            let expected = match join {
                LineJoin::Miter => 50.0 - 10.0 * 2.0f64.sqrt(),
                LineJoin::Round => 40.0,
                LineJoin::Bevel => 50.0 - 10.0 / 2.0f64.sqrt(),
            };
            assert!((b.y0 - expected).abs() < 0.02, "{join:?}: {b:?}");
        }
        st.join = LineJoin::Miter;
        st.miter_limit = 1.0;
        let limited = style.strip(&corner, Affine::IDENTITY, &st, 64).unwrap().outline.bounding_box();
        assert!((limited.y0 - (50.0 - 10.0 / 2.0f64.sqrt())).abs() < 0.02);
        let closed = NurbsPath::with_topology(vec![cp(20.0, 20.0), cp(80.0, 20.0), cp(80.0, 80.0), cp(20.0, 80.0)], vec![], true).unwrap();
        st.cap = LineCap::Butt;
        let a = style.strip(&closed, Affine::IDENTITY, &st, 64).unwrap();
        st.cap = LineCap::Round;
        let b = style.strip(&closed, Affine::IDENTITY, &st, 64).unwrap();
        assert_eq!(a.outline, b.outline);
    }
}
