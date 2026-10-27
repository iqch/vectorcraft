//! NURBS contours: clamped pieces joined at shared corner controls, or a smooth
//! periodic B-spline. Cached flattened controls are derived, never deserialized.
use super::{ControlPoint, MAX_CONTROLS, NurbsCurve, NurbsError};
use kurbo::{BezPath, PathEl, Point};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PathRepr", into = "PathRepr")]
pub struct NurbsPath {
    pieces: Vec<NurbsCurve>,
    periodic: bool,
    controls: Vec<ControlPoint>,
    corners: Vec<usize>,
    pasted: Vec<NurbsPaste>,
    control_start: usize,
    parameter_start: f64,
}
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum PathRepr {
    Joined {
        pieces: Vec<NurbsCurve>,
        periodic: bool,
        #[serde(default)]
        control_start: usize,
        #[serde(default)]
        parameter_start: f64,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pasted: Vec<NurbsPaste>,
    },
    // Existing NURBS objects used this exact curve representation.
    Legacy(NurbsCurve),
}
impl TryFrom<PathRepr> for NurbsPath {
    type Error = NurbsError;
    fn try_from(value: PathRepr) -> Result<Self, NurbsError> {
        match value {
            PathRepr::Legacy(c) => Self::from_curve(c),
            PathRepr::Joined { pieces, periodic, pasted, control_start, parameter_start } => {
                Self::from_parts(pieces, periodic)?.ordered(control_start, parameter_start)?.with_pastes(pasted)
            }
        }
    }
}
impl From<NurbsPath> for PathRepr {
    fn from(value: NurbsPath) -> Self {
        if value.control_start == 0
            && value.parameter_start == 0.0
            && value.pasted.is_empty()
            && value.pieces.len() == 1
            && (!value.periodic || value.pieces.first().is_some_and(NurbsCurve::is_periodic))
        {
            if let Some(curve) = value.pieces.first() {
                return Self::Legacy(curve.clone());
            }
        }
        Self::Joined {
            pieces: value.pieces,
            periodic: value.periodic,
            pasted: value.pasted,
            control_start: value.control_start,
            parameter_start: value.parameter_start,
        }
    }
}
impl NurbsPath {
    pub fn from_controls(controls: Vec<ControlPoint>) -> Result<Self, NurbsError> {
        Self::from_curve(NurbsCurve::from_controls(controls)?)
    }
    pub fn from_curve(curve: NurbsCurve) -> Result<Self, NurbsError> {
        let periodic = curve.is_periodic();
        Self::from_parts(vec![curve], periodic)
    }
    pub fn from_parts(pieces: Vec<NurbsCurve>, periodic: bool) -> Result<Self, NurbsError> {
        if pieces.is_empty() || pieces.len() > MAX_CONTROLS {
            return Err(NurbsError::Size);
        }
        if pieces.iter().any(NurbsCurve::is_periodic) {
            if pieces.len() != 1 || !periodic {
                return Err(NurbsError::Join);
            }
            let controls = pieces.first().ok_or(NurbsError::Size)?.controls().to_vec();
            return Ok(Self { pieces, periodic, controls, corners: vec![], pasted: vec![], control_start: 0, parameter_start: 0.0 });
        }
        let mut controls = Vec::new();
        let mut corners = Vec::new();
        for (i, piece) in pieces.iter().enumerate() {
            if i > 0 {
                if controls.last() != piece.controls().first() {
                    return Err(NurbsError::Join);
                }
                corners.push(controls.len() - 1);
            }
            controls.extend(piece.controls().iter().skip(usize::from(i > 0)).copied());
            if controls.len() > MAX_CONTROLS + usize::from(periodic) {
                return Err(NurbsError::Size);
            }
        }
        if periodic {
            if controls.first() != controls.last() {
                return Err(NurbsError::Join);
            }
            controls.pop();
            corners.insert(0, 0);
        }
        if controls.len() < 2 {
            return Err(NurbsError::Size);
        }
        Ok(Self { pieces, periodic, controls, corners, pasted: vec![], control_start: 0, parameter_start: 0.0 })
    }
    /// Topology changes may reshape the curve; each stretch has its own automatic degree.
    pub fn with_topology(controls: Vec<ControlPoint>, mut corners: Vec<usize>, periodic: bool) -> Result<Self, NurbsError> {
        let n = controls.len();
        if !(2..=MAX_CONTROLS).contains(&n) {
            return Err(NurbsError::Size);
        }
        if corners.iter().any(|i| *i >= n) {
            return Err(NurbsError::Index);
        }
        corners.sort_unstable();
        corners.dedup();
        if corners.is_empty() {
            return Self::from_curve(if periodic { NurbsCurve::periodic_from_controls(controls)? } else { NurbsCurve::from_controls(controls)? });
        }
        let start = if periodic { *corners.first().ok_or(NurbsError::Index)? } else { 0 };
        let mut controls = controls;
        if periodic {
            let start = *corners.first().ok_or(NurbsError::Index)?;
            controls.rotate_left(start);
            corners = corners.into_iter().map(|i| i - start).collect();
            controls.push(*controls.first().ok_or(NurbsError::Size)?);
            corners.push(n);
        } else {
            corners.retain(|i| *i > 0 && *i < n - 1);
            corners.insert(0, 0);
            corners.push(n - 1);
        }
        let pieces = corners
            .windows(2)
            .map(|w| {
                let a = *w.first().ok_or(NurbsError::Index)?;
                let b = *w.get(1).ok_or(NurbsError::Index)?;
                NurbsCurve::from_controls(controls.get(a..=b).ok_or(NurbsError::Index)?.to_vec())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let result = Self::from_parts(pieces, periodic)?;
        let phase = result.phase_for_control((n - start) % n);
        result.ordered(start, if periodic { phase } else { 0.0 })
    }
    pub fn controls(&self) -> &[ControlPoint] {
        &self.controls
    }
    pub fn pieces(&self) -> &[NurbsCurve] {
        &self.pieces
    }
    pub fn degree(&self) -> usize {
        self.pieces.iter().map(NurbsCurve::degree).max().unwrap_or(1)
    }
    pub fn is_periodic(&self) -> bool {
        self.periodic
    }
    pub fn is_corner(&self, index: usize) -> bool {
        self.corners.contains(&index)
    }
    pub fn corners(&self) -> &[usize] {
        &self.corners
    }
    pub fn knots(&self) -> Vec<f64> {
        let count = self.pieces.len() as f64;
        let mut out = vec![0.0, 1.0];
        for (i, piece) in self.pieces.iter().enumerate() {
            for &u in piece.knots().iter().filter(|u| **u >= 0.0 && **u <= 1.0) {
                let t = (i as f64 + u) / count;
                out.push(if self.periodic && self.parameter_start != 0.0 { (t - self.parameter_start).rem_euclid(1.0) } else { t });
            }
        }
        out.sort_by(f64::total_cmp);
        out
    }
    fn parameter(&self, u: f64) -> Result<(usize, f64), NurbsError> {
        if !u.is_finite() || !(0.0..=1.0).contains(&u) {
            return Err(NurbsError::Parameter);
        }
        let u = if self.periodic && self.parameter_start != 0.0 { (u + self.parameter_start).rem_euclid(1.0) } else { u };
        let value = u * self.pieces.len() as f64;
        let i = (value.floor() as usize).min(self.pieces.len() - 1);
        Ok((i, value - i as f64))
    }
    pub fn evaluate_color(&self, u: f64, hsv: bool) -> Result<[f64; 4], NurbsError> {
        let (i, t) = self.parameter(u)?;
        self.pieces.get(i).ok_or(NurbsError::Index)?.evaluate_color(t, hsv)
    }
    pub fn evaluate(&self, u: f64) -> Result<Point, NurbsError> {
        let (i, t) = self.parameter(u)?;
        self.pieces.get(i).ok_or(NurbsError::Index)?.evaluate(t)
    }
    pub fn to_bezpath(&self, tolerance: f64) -> Result<BezPath, NurbsError> {
        if !self.pasted.is_empty() {
            return self.composed_bezpath(tolerance);
        }
        self.base_bezpath(tolerance)
    }
    fn base_bezpath(&self, tolerance: f64) -> Result<BezPath, NurbsError> {
        let mut out = BezPath::new();
        for (i, piece) in self.pieces.iter().enumerate() {
            let bp = piece.to_bezpath(tolerance)?;
            for el in bp.elements() {
                if matches!(el, PathEl::ClosePath) || (i > 0 && matches!(el, PathEl::MoveTo(_))) {
                    continue;
                }
                if out.elements().len() >= 32768 {
                    return Err(NurbsError::Size);
                }
                out.push(*el);
            }
        }
        if self.periodic {
            out.close_path();
        }
        Ok(out)
    }
    pub fn with_periodic(&self, periodic: bool) -> Result<Self, NurbsError> {
        if self.periodic == periodic {
            return Ok(self.clone());
        }
        self.keep_pastes(Self::with_topology(self.controls.clone(), self.corners.clone(), periodic))
    }
    /// Corner is a shared clamped control. Clearing it merges its adjacent pieces.
    fn toggle_corner_raw(&self, index: usize) -> Result<Self, NurbsError> {
        if index >= self.controls.len() {
            return Err(NurbsError::Index);
        }
        if !self.periodic && (index == 0 || index + 1 == self.controls.len()) {
            return Err(NurbsError::Index);
        }
        if self.corners.is_empty() && self.periodic {
            return self.keep_pastes(Self::with_topology(self.controls.clone(), vec![index], true));
        }
        let mut pieces = self.pieces.clone();
        if self.is_corner(index) {
            if self.periodic && self.corners.len() == 1 {
                return self.keep_pastes(Self::with_topology(self.controls.clone(), vec![], true));
            }
            if self.periodic && index == 0 {
                let joined = pieces.last().ok_or(NurbsError::Index)?.join(pieces.first().ok_or(NurbsError::Index)?)?;
                let mut output = vec![joined];
                output.extend(pieces.get(1..pieces.len() - 1).ok_or(NurbsError::Index)?.iter().cloned());
                return self.keep_pastes(Self::from_parts(output, true));
            }
            let mut offset = 0;
            for i in 0..pieces.len() - 1 {
                offset += pieces.get(i).ok_or(NurbsError::Index)?.controls().len() - 1;
                if offset == index {
                    let merged = pieces.get(i).ok_or(NurbsError::Index)?.join(pieces.get(i + 1).ok_or(NurbsError::Index)?)?;
                    pieces.splice(i..=i + 1, [merged]);
                    return self.keep_pastes(Self::from_parts(pieces, self.periodic));
                }
            }
            return Err(NurbsError::Index);
        }
        let mut offset = 0;
        for i in 0..pieces.len() {
            let piece = pieces.get(i).ok_or(NurbsError::Index)?;
            let count = piece.controls().len();
            if index > offset && index < offset + count - 1 {
                let local = index - offset;
                let left = NurbsCurve::from_controls(piece.controls().get(..=local).ok_or(NurbsError::Index)?.to_vec())?;
                let right = NurbsCurve::from_controls(piece.controls().get(local..).ok_or(NurbsError::Index)?.to_vec())?;
                pieces.splice(i..=i, [left, right]);
                return self.keep_pastes(Self::from_parts(pieces, self.periodic));
            }
            offset += count - 1;
        }
        Err(NurbsError::Index)
    }
    pub fn set_control(&self, index: usize, control: ControlPoint) -> Result<Self, NurbsError> {
        if index >= self.controls.len() {
            return Err(NurbsError::Index);
        }
        if self.is_corner(index) && self.controls.get(index).is_some_and(|p| p.weight != control.weight) {
            return Err(NurbsError::Control);
        }
        let index = self.raw_index(index)?;
        let mut pieces = self.pieces.clone();
        let mut offset = 0;
        for piece in &mut pieces {
            let count = piece.controls().len();
            if index >= offset && index < offset + count {
                *piece = piece.set_control(index - offset, control)?;
            }
            offset += count - 1;
        }
        if self.periodic && !self.corners.is_empty() && index == 0 {
            let last = pieces.last_mut().ok_or(NurbsError::Index)?;
            *last = last.set_control(last.controls().len() - 1, control)?;
        }
        self.keep_pastes(Self::from_parts(pieces, self.periodic)?.ordered(self.control_start, self.parameter_start))
    }
    pub fn append_control(&self, control: ControlPoint) -> Result<Self, NurbsError> {
        if self.controls.len() >= MAX_CONTROLS {
            return Err(NurbsError::Size);
        }
        if self.periodic {
            let mut controls = self.controls.clone();
            controls.push(control);
            return self.keep_pastes(Self::with_topology(controls, self.corners.clone(), true));
        }
        let mut pieces = self.pieces.clone();
        let last = pieces.last_mut().ok_or(NurbsError::Size)?;
        *last = last.append_control(control)?;
        self.keep_pastes(Self::from_parts(pieces, false))
    }
    fn remove_control_raw(&self, index: usize) -> Result<Self, NurbsError> {
        if index >= self.controls.len() {
            return Err(NurbsError::Index);
        }
        if self.is_corner(index) {
            // Deleting a corner is a topology edit: remove it and rejoin the surviving controls.
            let mut controls = self.controls.clone();
            controls.remove(index);
            let corners = self.corners.iter().copied().filter(|i| *i != index).map(|i| if i > index { i - 1 } else { i }).collect();
            return self.keep_pastes(Self::with_topology(controls, corners, self.periodic));
        }
        let mut pieces = self.pieces.clone();
        let mut offset = 0;
        for i in 0..pieces.len() {
            let piece = pieces.get(i).ok_or(NurbsError::Index)?;
            let count = piece.controls().len();
            if index >= offset && index < offset + count {
                if count == 2 && pieces.len() > 1 {
                    if i == 0 && index == 0 {
                        pieces.remove(0);
                        return self.keep_pastes(Self::from_parts(pieces, false));
                    }
                    if i + 1 == pieces.len() && index + 1 == self.controls.len() {
                        pieces.pop();
                        return self.keep_pastes(Self::from_parts(pieces, false));
                    }
                }
                let new = piece.remove_control(index - offset)?;
                *pieces.get_mut(i).ok_or(NurbsError::Index)? = new;
                return self.keep_pastes(Self::from_parts(pieces, self.periodic));
            }
            offset += count - 1;
        }
        Err(NurbsError::Index)
    }
    pub fn insert_vertex(&self, u: f64) -> Result<Self, NurbsError> {
        let (i, t) = self.parameter(u)?;
        let mut pieces = self.pieces.clone();
        let piece = pieces.get_mut(i).ok_or(NurbsError::Index)?;
        *piece = piece.insert_vertex(t)?;
        let result = Self::from_parts(pieces, self.periodic)?;
        if self.control_start == 0 {
            self.keep_pastes(result.ordered(0, self.parameter_start))
        } else {
            self.restore_order(result, *self.controls.first().ok_or(NurbsError::Size)?, Some(self.parameter_start))
        }
    }
}

#[cfg(test)]
#[path = "path/tests.rs"]
mod tests;

#[path = "path/order.rs"]
mod order;
#[path = "path/paste.rs"]
mod paste;
pub use paste::{NurbsPaste, PasteAnchor, PasteMode};
