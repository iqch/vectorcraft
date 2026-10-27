//! Editable, positive-weight, clamped NURBS in a normalized parameter domain.
//!
//! Original implementation from the mathematical definitions of de Boor evaluation and
//! homogeneous knot insertion: https://pages.mtu.edu/~shene/COURSES/cs3621/NOTES/spline/NURBS-knot-insert.html
//! Refinement preserves degree; topology edits rebuild an automatic-degree uniform curve.
//! Periodic construction reference: https://pages.mtu.edu/~shene/COURSES/cs3621/NOTES/spline/B-spline/bspline-curve.html

use kurbo::{BezPath, Point};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Upper bound on controls in a single editable curve.
pub const MAX_CONTROLS: usize = 4096;
pub mod debris;
#[path = "nurbs/path.rs"]
pub mod path;
pub use path::{NurbsPaste, NurbsPath, PasteAnchor, PasteMode};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ControlPoint {
    pub position: Point,
    pub weight: f64,
    /// Unassociated display RGBA. White preserves legacy appearance.
    #[serde(default = "white", skip_serializing_if = "is_white")]
    pub color: [f64; 4],
}

fn white() -> [f64; 4] {
    [1.0; 4]
}
fn is_white(c: &[f64; 4]) -> bool {
    *c == white()
}

impl ControlPoint {
    pub fn new(position: Point, weight: f64) -> Self {
        Self { position, weight, color: white() }
    }

    pub(super) fn valid(self) -> bool {
        self.position.x.is_finite()
            && self.position.y.is_finite()
            && self.position.x.abs() <= 1e100
            && self.position.y.abs() <= 1e100
            && self.weight.is_finite()
            && (1e-100..=1e100).contains(&self.weight)
            && self.color.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
    }

    // Interpolation in homogeneous coordinates, normalized before multiplying coordinates.
    pub(super) fn blend(self, other: Self, alpha: f64) -> Result<Self, NurbsError> {
        if !alpha.is_finite() || !(0.0..=1.0).contains(&alpha) {
            return Err(NurbsError::Numerical);
        }
        let a = (1.0 - alpha) * self.weight;
        let b = alpha * other.weight;
        let weight = a + b;
        let position = Point::new(
            self.position.x * (a / weight) + other.position.x * (b / weight),
            self.position.y * (a / weight) + other.position.y * (b / weight),
        );
        let color = std::array::from_fn(|i| (self.color[i] * (a / weight) + other.color[i] * (b / weight)).clamp(0.0, 1.0));
        let result = Self { position, weight, color };
        if result.valid() { Ok(result) } else { Err(NurbsError::Numerical) }
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum NurbsError {
    #[error("invalid pasted NURBS: child must be open, named, attached at both ends and occupy a non-overlapping interval")]
    Paste,
    #[error("a NURBS curve needs 2..=4096 control points and degree 1..=3 below its point count")]
    Size,
    #[error("NURBS coordinates, positive weights or RGBA colors are outside the supported finite range (colors must be 0..1)")]
    Control,
    #[error("invalid normalized clamped knot vector")]
    Knots,
    #[error("parameter must be finite and within the curve domain (strictly inside for refinement)")]
    Parameter,
    #[error("knot multiplicity already equals the degree")]
    Multiplicity,
    #[error("control or corner index is out of range")]
    Index,
    #[error("adjacent curve endpoints must coincide")]
    Join,
    #[error("NURBS computation exceeds numerical limits")]
    Numerical,
}

/// Private fields ensure that every evaluation operates on validated data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "NurbsRepr", into = "NurbsRepr")]
pub struct NurbsCurve {
    degree: usize,
    controls: Vec<ControlPoint>,
    knots: Vec<f64>,
    periodic: bool,
}

#[derive(Serialize, Deserialize)]
struct NurbsRepr {
    degree: usize,
    controls: Vec<ControlPoint>,
    knots: Vec<f64>,
    #[serde(default, skip_serializing_if = "is_false")]
    periodic: bool,
}
fn is_false(value: &bool) -> bool {
    !*value
}
impl TryFrom<NurbsRepr> for NurbsCurve {
    type Error = NurbsError;
    fn try_from(value: NurbsRepr) -> Result<Self, Self::Error> {
        if value.periodic {
            let n = value.controls.len();
            let end = value.degree.checked_add(n).and_then(|v| v.checked_add(1)).ok_or(NurbsError::Size)?;
            let breaks = value.knots.get(value.degree..end).ok_or(NurbsError::Knots)?.to_vec();
            let curve = Self::new_periodic(value.degree, value.controls, breaks)?;
            if curve.knots != value.knots {
                return Err(NurbsError::Knots);
            }
            Ok(curve)
        } else {
            Self::new(value.degree, value.controls, value.knots)
        }
    }
}
impl From<NurbsCurve> for NurbsRepr {
    fn from(value: NurbsCurve) -> Self {
        Self { degree: value.degree, controls: value.controls, knots: value.knots, periodic: value.periodic }
    }
}
impl NurbsCurve {
    /// Automatic degree min(control_count - 1, 3), with uniform interior knots.
    pub fn from_controls(controls: Vec<ControlPoint>) -> Result<Self, NurbsError> {
        let count = controls.len();
        if !(2..=MAX_CONTROLS).contains(&count) {
            return Err(NurbsError::Size);
        }
        let degree = (count - 1).min(3);
        let mut knots = vec![0.0; degree + 1];
        for i in 1..count - degree {
            knots.push(i as f64 / (count - degree) as f64);
        }
        knots.extend(std::iter::repeat_n(1.0, degree + 1));
        Self::new(degree, controls, knots)
    }

    /// Accepts clamped knots on [0,1], with interior multiplicity at most degree.
    pub fn new(degree: usize, controls: Vec<ControlPoint>, knots: Vec<f64>) -> Result<Self, NurbsError> {
        let n = controls.len();
        if !(2..=MAX_CONTROLS).contains(&n) || !(1..=3).contains(&degree) || degree >= n {
            return Err(NurbsError::Size);
        }
        if controls.iter().any(|p| !p.valid()) {
            return Err(NurbsError::Control);
        }
        if knots.len() != n + degree + 1
            || knots.iter().any(|u| !u.is_finite() || !(0.0..=1.0).contains(u))
            || knots.windows(2).any(|w| w.first() > w.get(1))
        {
            return Err(NurbsError::Knots);
        }
        let mut run = 0;
        let mut previous = None;
        for (i, &u) in knots.iter().enumerate() {
            if (i <= degree && u != 0.0) || (i >= n && u != 1.0) || (i > degree && i < n && (u == 0.0 || u == 1.0)) {
                return Err(NurbsError::Knots);
            }
            run = if previous == Some(u) { run + 1 } else { 1 };
            if u > 0.0 && u < 1.0 && run > degree {
                return Err(NurbsError::Knots);
            }
            previous = Some(u);
        }
        Ok(Self { degree, controls, knots, periodic: false })
    }

    /// Bounded display approximation; polynomial spans remain exact cubics.
    pub fn to_bezpath(&self, tolerance: f64) -> Result<BezPath, NurbsError> {
        if !tolerance.is_finite() || tolerance <= 0.0 {
            return Err(NurbsError::Parameter);
        }
        if self.periodic {
            let mut path = self.as_clamped()?.to_bezpath(tolerance)?;
            path.close_path();
            return Ok(path);
        }
        let cubic = self.to_cubic()?;
        let mut refined = cubic.clone();
        let mut breaks = cubic.knots.iter().copied().filter(|u| *u > 0.0 && *u < 1.0).collect::<Vec<_>>();
        breaks.dedup();
        for u in breaks {
            while refined.knots.iter().filter(|&&v| v == u).count() < 3 {
                refined = refined.insert_knot(u)?;
            }
        }
        let mut path = BezPath::new();
        path.move_to(refined.control(0)?.position);
        let mut stack = Vec::new();
        for i in (0..refined.controls.len() - 1).step_by(3) {
            stack.push(([refined.control(i)?, refined.control(i + 1)?, refined.control(i + 2)?, refined.control(i + 3)?], 0));
            while let Some(([a, b, c, d], depth)) = stack.pop() {
                if path.elements().len() >= 32768 {
                    return Err(NurbsError::Size);
                }
                if a.weight == b.weight && a.weight == c.weight && a.weight == d.weight {
                    path.curve_to(b.position, c.position, d.position);
                    continue;
                }
                let chord = d.position - a.position;
                let length = chord.hypot();
                let distance = |p: Point| {
                    if length == 0.0 {
                        p.distance(a.position)
                    } else {
                        let t = ((p - a.position).dot(chord) / length / length).clamp(0.0, 1.0);
                        p.distance(a.position + chord * t)
                    }
                };
                if distance(b.position).max(distance(c.position)) <= tolerance {
                    path.line_to(d.position);
                } else {
                    if depth >= 24 {
                        return Err(NurbsError::Numerical);
                    }
                    let ab = a.blend(b, 0.5)?;
                    let bc = b.blend(c, 0.5)?;
                    let cd = c.blend(d, 0.5)?;
                    let abc = ab.blend(bc, 0.5)?;
                    let bcd = bc.blend(cd, 0.5)?;
                    let mid = abc.blend(bcd, 0.5)?;
                    stack.push(([mid, bcd, cd, d], depth + 1));
                    stack.push(([a, ab, abc, mid], depth + 1));
                }
            }
        }
        Ok(path)
    }
    pub fn degree(&self) -> usize {
        self.degree
    }
    pub fn controls(&self) -> &[ControlPoint] {
        &self.controls
    }
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }
    fn control(&self, i: usize) -> Result<ControlPoint, NurbsError> {
        self.controls.get(i).copied().ok_or(NurbsError::Index)
    }
    fn knot(&self, i: usize) -> Result<f64, NurbsError> {
        self.knots.get(i).copied().ok_or(NurbsError::Knots)
    }
    fn span(&self, u: f64) -> usize {
        self.knots.partition_point(|k| *k <= u).saturating_sub(1).min(self.controls.len() + if self.periodic { self.degree } else { 0 } - 1)
    }
    fn interior(u: f64) -> Result<(), NurbsError> {
        if u.is_finite() && u > 0.0 && u < 1.0 { Ok(()) } else { Err(NurbsError::Parameter) }
    }

    pub fn evaluate(&self, u: f64) -> Result<Point, NurbsError> {
        Ok(self.evaluate_control(u)?.position)
    }
    pub fn evaluate_control(&self, u: f64) -> Result<ControlPoint, NurbsError> {
        if !u.is_finite() || !(0.0..=1.0).contains(&u) {
            return Err(NurbsError::Parameter);
        }
        let u = if self.periodic && u == 1.0 { 0.0 } else { u };
        if !self.periodic && u == 0.0 {
            return self.control(0);
        }
        if !self.periodic && u == 1.0 {
            return self.control(self.controls.len() - 1);
        }
        let k = self.span(u);
        let p = self.degree;
        let mut work = (0..=p).map(|j| self.control((k - p + j) % self.controls.len())).collect::<Result<Vec<_>, _>>()?;
        for r in 1..=p {
            for j in (r..=p).rev() {
                let i = k - p + j;
                let lo = self.knot(i)?;
                let hi = self.knot(i + p + 1 - r)?;
                if hi <= lo {
                    return Err(NurbsError::Numerical);
                }
                let a = *work.get(j - 1).ok_or(NurbsError::Index)?;
                let b = *work.get(j).ok_or(NurbsError::Index)?;
                *work.get_mut(j).ok_or(NurbsError::Index)? = a.blend(b, (u - lo) / (hi - lo))?;
            }
        }
        work.last().copied().ok_or(NurbsError::Index)
    }

    /// Shape-preserving knot insertion at fixed degree. Returns a new curve atomically.
    pub fn insert_knot(&self, u: f64) -> Result<Self, NurbsError> {
        if self.periodic {
            return self.insert_periodic(u);
        }
        Self::interior(u)?;
        if self.controls.len() >= MAX_CONTROLS {
            return Err(NurbsError::Size);
        }
        let s = self.knots.iter().filter(|&&v| v == u).count();
        let p = self.degree;
        if s >= p {
            return Err(NurbsError::Multiplicity);
        }
        let k = self.span(u);
        let mut controls = Vec::with_capacity(self.controls.len() + 1);
        for i in 0..=self.controls.len() {
            controls.push(if i <= k - p {
                self.control(i)?
            } else if i > k - s {
                self.control(i - 1)?
            } else {
                let lo = self.knot(i)?;
                let hi = self.knot(i + p)?;
                if hi <= lo {
                    return Err(NurbsError::Numerical);
                }
                self.control(i - 1)?.blend(self.control(i)?, (u - lo) / (hi - lo))?
            });
        }
        let mut knots = self.knots.clone();
        knots.insert(k + 1, u);
        Self::new(p, controls, knots)
    }

    /// Raise each polynomial span to cubic in homogeneous coordinates, preserving shape.
    /// Low-degree curves can need several additional controls, not just one.
    pub fn to_cubic(&self) -> Result<Self, NurbsError> {
        if self.periodic && self.degree != 3 {
            return Self::periodic_from_clamped(self.as_clamped()?.to_cubic()?);
        }
        if self.degree == 3 {
            return Ok(self.clone());
        }
        let breaks: Vec<_> = self.knots.iter().copied().filter(|u| *u > 0.0 && *u < 1.0).fold(Vec::new(), |mut result, u| {
            if result.last() != Some(&u) {
                result.push(u);
            }
            result
        });
        if (breaks.len() + 1) * 3 + 1 > MAX_CONTROLS {
            return Err(NurbsError::Size);
        }
        let mut refined = self.clone();
        for &u in &breaks {
            while refined.knots.iter().filter(|&&v| v == u).count() < self.degree {
                refined = refined.insert_knot(u)?;
            }
        }
        let mut controls = Vec::new();
        for span in 0..=breaks.len() {
            let start = span * self.degree;
            let mut piece = refined.controls.get(start..=start + self.degree).ok_or(NurbsError::Index)?.to_vec();
            for degree in self.degree..3 {
                let mut raised = vec![*piece.first().ok_or(NurbsError::Index)?];
                for i in 1..=degree {
                    let a = *piece.get(i - 1).ok_or(NurbsError::Index)?;
                    let b = *piece.get(i).ok_or(NurbsError::Index)?;
                    raised.push(a.blend(b, 1.0 - i as f64 / (degree + 1) as f64)?);
                }
                raised.push(*piece.last().ok_or(NurbsError::Index)?);
                piece = raised;
            }
            controls.extend(piece.into_iter().skip(usize::from(span > 0)));
        }
        let mut knots = vec![0.0; 4];
        for u in breaks {
            knots.extend(std::iter::repeat_n(u, 3));
        }
        knots.extend(std::iter::repeat_n(1.0, 4));
        Self::new(3, controls, knots)
    }

    /// Shape-preserving editor refinement with automatic degree. Raising a short curve
    /// to cubic may add extra controls. At an existing full-multiplicity joint it is
    /// necessary to choose another parameter or split the curve instead.
    pub fn insert_vertex(&self, u: f64) -> Result<Self, NurbsError> {
        if !self.periodic {
            Self::interior(u)?;
        }
        self.to_cubic()?.insert_knot(u)
    }
    /// Split into clamped curves, each reparameterized to [0,1]. Their shared end is a corner.
    pub fn split(&self, u: f64) -> Result<(Self, Self), NurbsError> {
        if self.periodic {
            return self.as_clamped()?.split(u);
        }
        Self::interior(u)?;
        let mut refined = self.clone();
        while refined.knots.iter().filter(|&&v| v == u).count() < self.degree {
            refined = refined.insert_knot(u)?;
        }
        let k = refined.span(u);
        let joint = k - self.degree;
        let left_controls = refined.controls.get(..=joint).ok_or(NurbsError::Index)?.to_vec();
        let right_controls = refined.controls.get(joint..).ok_or(NurbsError::Index)?.to_vec();
        let mut left_knots: Vec<_> = refined.knots.iter().copied().filter(|v| *v < u).map(|v| v / u).collect();
        left_knots.extend(std::iter::repeat_n(1.0, self.degree + 1));
        let mut right_knots = vec![0.0; self.degree + 1];
        right_knots.extend(refined.knots.iter().copied().filter(|v| *v > u).map(|v| (v - u) / (1.0 - u)));
        Ok((Self::new(self.degree, left_controls, left_knots)?, Self::new(self.degree, right_controls, right_knots)?))
    }

    /// Move a control or change its weight without changing knots.
    pub fn set_control(&self, index: usize, control: ControlPoint) -> Result<Self, NurbsError> {
        if !control.valid() {
            return Err(NurbsError::Control);
        }
        let mut result = self.clone();
        *result.controls.get_mut(index).ok_or(NurbsError::Index)? = control;
        Ok(result)
    }

    /// Topology edit: rebuild uniform knots and automatic degree; shape may change.
    pub fn remove_control(&self, index: usize) -> Result<Self, NurbsError> {
        if index >= self.controls.len() {
            return Err(NurbsError::Index);
        }
        let mut controls = self.controls.clone();
        controls.remove(index);
        if self.periodic { Self::periodic_from_controls(controls) } else { Self::from_controls(controls) }
    }

    /// Topology edit: append a control and rebuild automatic degree and uniform knots.
    pub fn append_control(&self, control: ControlPoint) -> Result<Self, NurbsError> {
        if self.controls.len() >= MAX_CONTROLS {
            return Err(NurbsError::Size);
        }
        let mut controls = self.controls.clone();
        controls.push(control);
        if self.periodic { Self::periodic_from_controls(controls) } else { Self::from_controls(controls) }
    }

    /// Remove a corner, keeping its common control once. Rebuilds knots; shape may change.
    pub fn join(&self, next: &Self) -> Result<Self, NurbsError> {
        let end = self.controls.last().ok_or(NurbsError::Size)?;
        if end.position != next.control(0)?.position {
            return Err(NurbsError::Join);
        }
        if self.controls.len() + next.controls.len() - 1 > MAX_CONTROLS {
            return Err(NurbsError::Size);
        }
        let mut controls = self.controls.clone();
        controls.extend(next.controls.iter().skip(1).copied());
        if self.periodic { Self::periodic_from_controls(controls) } else { Self::from_controls(controls) }
    }
}

#[cfg(test)]
#[path = "nurbs/tests.rs"]
mod tests;

// Periodic algorithms use an extended cyclic knot/control sequence. The copies are
// temporary: only independent controls and one normalized period are saved.
impl NurbsCurve {
    pub fn is_periodic(&self) -> bool {
        self.periodic
    }
    pub fn periodic_from_controls(controls: Vec<ControlPoint>) -> Result<Self, NurbsError> {
        let n = controls.len();
        if !(2..=MAX_CONTROLS).contains(&n) {
            return Err(NurbsError::Size);
        }
        let breaks = (0..=n).map(|i| i as f64 / n as f64).collect();
        Self::new_periodic((n - 1).min(3), controls, breaks)
    }
    fn new_periodic(degree: usize, controls: Vec<ControlPoint>, breaks: Vec<f64>) -> Result<Self, NurbsError> {
        let n = controls.len();
        if !(2..=MAX_CONTROLS).contains(&n) || !(1..=3).contains(&degree) || degree >= n {
            return Err(NurbsError::Size);
        }
        if controls.iter().any(|p| !p.valid()) {
            return Err(NurbsError::Control);
        }
        if breaks.len() != n + 1
            || breaks.first() != Some(&0.0)
            || breaks.last() != Some(&1.0)
            || breaks.iter().any(|u| !u.is_finite() || !(0.0..=1.0).contains(u))
            || breaks.get(..n).ok_or(NurbsError::Knots)?.contains(&1.0)
            || breaks.windows(2).any(|w| w.first() > w.get(1))
        {
            return Err(NurbsError::Knots);
        }
        let mut run = 0;
        let mut previous = None;
        for &u in &breaks {
            run = if previous == Some(u) { run + 1 } else { 1 };
            if run > degree {
                return Err(NurbsError::Multiplicity);
            }
            previous = Some(u);
        }
        let knots = cyclic_knots(&breaks, degree, n + 2 * degree + 1, 0.0)?;
        Ok(Self { degree, controls, knots, periodic: true })
    }
    fn period_breaks(&self) -> Result<Vec<f64>, NurbsError> {
        Ok(self.knots.get(self.degree..=self.degree + self.controls.len()).ok_or(NurbsError::Knots)?.to_vec())
    }
    /// Exact clamping by knot insertion, for rendering and degree elevation only.
    pub fn as_clamped(&self) -> Result<Self, NurbsError> {
        if !self.periodic {
            return Ok(self.clone());
        }
        let mut controls = self.controls.clone();
        for i in 0..self.degree {
            controls.push(self.control(i)?);
        }
        let mut knots = self.knots.clone();
        for u in [0.0, 1.0] {
            while knots.iter().filter(|&&k| k == u).count() < self.degree + 1 {
                insert_raw(self.degree, &mut controls, &mut knots, u)?;
            }
        }
        let first = knots.iter().position(|u| *u == 0.0).ok_or(NurbsError::Knots)?;
        let last = knots.iter().rposition(|u| *u == 1.0).ok_or(NurbsError::Knots)?;
        let knots = knots.get(first..=last).ok_or(NurbsError::Knots)?.to_vec();
        let count = knots.len().checked_sub(self.degree + 1).ok_or(NurbsError::Knots)?;
        let mut controls = controls.get(first..first + count).ok_or(NurbsError::Index)?.to_vec();
        let start = *controls.first().ok_or(NurbsError::Size)?;
        *controls.last_mut().ok_or(NurbsError::Size)? = start;
        Self::new(self.degree, controls, knots)
    }
    fn periodic_from_clamped(curve: Self) -> Result<Self, NurbsError> {
        let p = curve.degree;
        let mut controls = curve.controls;
        if controls.first() != controls.last() {
            return Err(NurbsError::Join);
        }
        controls.pop();
        controls.rotate_right(p - 1);
        let breaks = curve.knots.get(1..curve.knots.len() - p).ok_or(NurbsError::Knots)?.to_vec();
        Self::new_periodic(p, controls, breaks)
    }
    fn insert_periodic(&self, u: f64) -> Result<Self, NurbsError> {
        if !u.is_finite() || !(0.0..=1.0).contains(&u) {
            return Err(NurbsError::Parameter);
        }
        let u = if u == 1.0 { 0.0 } else { u };
        let n = self.controls.len();
        if n >= MAX_CONTROLS {
            return Err(NurbsError::Size);
        }
        let mut breaks = self.period_breaks()?;
        if breaks.iter().filter(|&&v| v == u).count() >= self.degree {
            return Err(NurbsError::Multiplicity);
        }
        let mut controls = (0..3 * n + self.degree).map(|i| self.control(i % n)).collect::<Result<Vec<_>, _>>()?;
        let mut knots = cyclic_knots(&breaks, self.degree, 3 * n + 2 * self.degree + 1, -1.0)?;
        for value in [u - 1.0, u, u + 1.0] {
            insert_raw(self.degree, &mut controls, &mut knots, value)?;
        }
        let first = knots.iter().position(|v| *v == 0.0).ok_or(NurbsError::Knots)?;
        let start = first.checked_sub(self.degree).ok_or(NurbsError::Knots)?;
        let controls = controls.get(start..start + n + 1).ok_or(NurbsError::Index)?.to_vec();
        breaks.insert(breaks.partition_point(|v| *v <= u), u);
        Self::new_periodic(self.degree, controls, breaks)
    }
}
fn cyclic_knots(breaks: &[f64], degree: usize, count: usize, offset: f64) -> Result<Vec<f64>, NurbsError> {
    let n = breaks.len().checked_sub(1).filter(|n| *n > 0).ok_or(NurbsError::Knots)? as isize;
    (0..count)
        .map(|j| {
            let i = j as isize - degree as isize;
            let u = *breaks.get(i.rem_euclid(n) as usize).ok_or(NurbsError::Knots)?;
            Ok(u + i.div_euclid(n) as f64 + offset)
        })
        .collect()
}
fn insert_raw(p: usize, controls: &mut Vec<ControlPoint>, knots: &mut Vec<f64>, u: f64) -> Result<(), NurbsError> {
    let n = controls.len();
    if knots.len() != n + p + 1 {
        return Err(NurbsError::Knots);
    }
    let k = knots.partition_point(|v| *v <= u).checked_sub(1).ok_or(NurbsError::Knots)?;
    let s = knots.iter().filter(|&&v| v == u).count();
    if k < p || k < s || s > p || k - p >= n || k - s >= n {
        return Err(NurbsError::Knots);
    }
    let mut output = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let q = if i <= k - p {
            *controls.get(i).ok_or(NurbsError::Index)?
        } else if i > k - s {
            *controls.get(i - 1).ok_or(NurbsError::Index)?
        } else {
            let lo = *knots.get(i).ok_or(NurbsError::Knots)?;
            let hi = *knots.get(i + p).ok_or(NurbsError::Knots)?;
            if hi <= lo {
                return Err(NurbsError::Numerical);
            }
            controls.get(i - 1).ok_or(NurbsError::Index)?.blend(*controls.get(i).ok_or(NurbsError::Index)?, (u - lo) / (hi - lo))?
        };
        output.push(q);
    }
    knots.insert(k + 1, u);
    *controls = output;
    Ok(())
}

#[path = "nurbs/color.rs"]
mod color;
pub use color::mix_color;
