//! RGBA attributes on rational controls; HSV takes the short hue arc.
use super::{NurbsCurve, NurbsError};
pub fn mix_color(a: [f64; 4], b: [f64; 4], t: f64, hsv: bool) -> [f64; 4] {
    let mut out = std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t);
    if hsv {
        let mut x = to_hsv(a);
        let mut y = to_hsv(b);
        if x[1] < 1e-12 {
            x[0] = y[0];
        }
        if y[1] < 1e-12 {
            y[0] = x[0];
        }
        let h = x[0] + ((y[0] - x[0] + 0.5).rem_euclid(1.0) - 0.5) * t;
        let s = x[1] + (y[1] - x[1]) * t;
        let v = x[2] + (y[2] - x[2]) * t;
        let k = h.rem_euclid(1.0) * 6.0;
        let f = k.fract();
        let p = v * (1.0 - s);
        let q = v * (1.0 - s * f);
        let r = v * (1.0 - s * (1.0 - f));
        let rgb = match k as u32 {
            0 => [v, r, p],
            1 => [q, v, p],
            2 => [p, v, r],
            3 => [p, q, v],
            4 => [r, p, v],
            _ => [v, p, q],
        };
        out[..3].copy_from_slice(&rgb);
    }
    out.map(|v| v.clamp(0.0, 1.0))
}
fn to_hsv(c: [f64; 4]) -> [f64; 3] {
    let [r, g, b, _] = c;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d < 1e-12 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    [h / 6.0, if max > 0.0 { d / max } else { 0.0 }, max]
}
impl NurbsCurve {
    pub fn evaluate_color(&self, u: f64, hsv: bool) -> Result<[f64; 4], NurbsError> {
        if !hsv {
            return Ok(self.evaluate_control(u)?.color);
        }
        if !u.is_finite() || !(0.0..=1.0).contains(&u) {
            return Err(NurbsError::Parameter);
        }
        let u = if self.periodic && u == 1.0 { 0.0 } else { u };
        if !self.periodic && (u == 0.0 || u == 1.0) {
            return Ok(self.control(if u == 0.0 { 0 } else { self.controls.len() - 1 })?.color);
        }
        let k = self.span(u);
        let p = self.degree;
        let mut work =
            (0..=p).map(|j| self.control((k - p + j) % self.controls.len()).map(|c| (c.color, c.weight))).collect::<Result<Vec<_>, _>>()?;
        for r in 1..=p {
            for j in (r..=p).rev() {
                let i = k - p + j;
                let lo = self.knot(i)?;
                let hi = self.knot(i + p + 1 - r)?;
                if hi <= lo {
                    return Err(NurbsError::Numerical);
                }
                let t = (u - lo) / (hi - lo);
                let (a, aw) = *work.get(j - 1).ok_or(NurbsError::Index)?;
                let (b, bw) = *work.get(j).ok_or(NurbsError::Index)?;
                let w = (1.0 - t) * aw + t * bw;
                *work.get_mut(j).ok_or(NurbsError::Index)? = (mix_color(a, b, t * bw / w, true), w);
            }
        }
        Ok(work.last().ok_or(NurbsError::Index)?.0)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::ControlPoint;
    use kurbo::Point;
    #[test]
    fn nurbs_rgba_refinement_and_hsv_short_arc() {
        let mut a = ControlPoint::new(Point::ZERO, 1.0);
        a.color = [1.0, 0.0, 0.1, 0.2];
        let mut b = ControlPoint::new(Point::new(100.0, 0.0), 2.0);
        b.color = [1.0, 0.1, 0.0, 0.8];
        let c = NurbsCurve::from_controls(vec![a, b]).unwrap();
        let refined = c.insert_knot(0.4).unwrap();
        for i in 0..=50 {
            let u = i as f64 / 50.0;
            for (x, y) in c.evaluate_color(u, false).unwrap().into_iter().zip(refined.evaluate_color(u, false).unwrap()) {
                assert!((x - y).abs() < 1e-10);
            }
        }
        let hsv = mix_color(a.color, b.color, 0.5, true);
        assert!(hsv[0] > 0.99 && hsv[1] < 0.001 && hsv[2] < 0.001);
        assert!((hsv[3] - 0.5).abs() < 1e-10);
        assert!(serde_json::from_value::<NurbsCurve>(serde_json::json!({"degree":1,"controls":[{"position":{"x":0,"y":0},"weight":1,"color":[2,0,0,1]},{"position":{"x":1,"y":0},"weight":1}],"knots":[0,0,1,1]})).is_err());
    }
}
