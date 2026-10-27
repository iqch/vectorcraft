//! Stable public control order and parameter seam, independent of internal clamped-piece seams.
use super::*;
impl NurbsPath {
    pub(super) fn raw_index(&self, index: usize) -> Result<usize, NurbsError> {
        let n = self.controls.len();
        if index >= n {
            return Err(NurbsError::Index);
        }
        Ok((index + n - self.control_start) % n)
    }
    pub(super) fn ordered(mut self, start: usize, phase: f64) -> Result<Self, NurbsError> {
        if start >= self.controls.len() || !phase.is_finite() || !(0.0..1.0).contains(&phase) || (!self.periodic && (start != 0 || phase != 0.0)) {
            return Err(NurbsError::Parameter);
        }
        self.controls.rotate_right(start);
        let n = self.controls.len();
        for i in &mut self.corners {
            *i = (*i + start) % n;
        }
        self.corners.sort_unstable();
        self.control_start = start;
        self.parameter_start = phase;
        Ok(self)
    }
    pub(super) fn phase_for_control(&self, index: usize) -> f64 {
        let mut offset = 0;
        for (i, piece) in self.pieces.iter().enumerate() {
            let span = piece.controls().len().saturating_sub(1);
            if index <= offset + span {
                return (i as f64 + (index - offset) as f64 / span.max(1) as f64) / self.pieces.len() as f64;
            }
            offset += span;
        }
        0.0
    }
    pub(super) fn restore_order(&self, result: Self, anchor: ControlPoint, phase: Option<f64>) -> Result<Self, NurbsError> {
        let raw = Self::from_parts(result.pieces, result.periodic)?;
        let index = raw.controls.iter().position(|p| *p == anchor).unwrap_or_else(|| {
            raw.controls
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| a.position.distance_squared(anchor.position).total_cmp(&b.position.distance_squared(anchor.position)))
                .map(|(i, _)| i)
                .unwrap_or(0)
        });
        if raw.periodic && raw.corners.is_empty() && phase.is_none() {
            let mut controls = raw.controls;
            controls.rotate_left(index);
            return self.keep_pastes(Self::with_topology(controls, vec![], true));
        }
        let start = (raw.controls.len() - index) % raw.controls.len();
        let phase = phase.unwrap_or_else(|| raw.phase_for_control(index));
        self.keep_pastes(raw.ordered(start, if self.periodic { phase.rem_euclid(1.0) } else { 0.0 }))
    }
    pub fn toggle_corner(&self, index: usize) -> Result<Self, NurbsError> {
        let local = self.raw_index(index)?;
        if self.periodic && self.corners.len() == 1 && self.is_corner(index) {
            return self.keep_pastes(Self::with_topology(self.controls.clone(), vec![], true));
        }
        let raw = Self::from_parts(self.pieces.clone(), self.periodic)?;
        let result = raw.toggle_corner_raw(local)?;
        self.restore_order(result, *self.controls.first().ok_or(NurbsError::Size)?, None)
    }
    pub fn remove_control(&self, index: usize) -> Result<Self, NurbsError> {
        let local = self.raw_index(index)?;
        let anchor = *self.controls.get(if index == 0 { 1 } else { 0 }).ok_or(NurbsError::Size)?;
        let raw = Self::from_parts(self.pieces.clone(), self.periodic)?;
        let result = raw.remove_control_raw(local)?;
        self.restore_order(result, anchor, None)
    }
}

impl NurbsPath {
    /// A full replacement may rebuild topology, but an existing corner's weight is locked.
    pub fn validate_corner_weights(&self, next: &Self) -> Result<(), NurbsError> {
        if self.controls.len() == next.controls.len() {
            for &i in &self.corners {
                if next.is_corner(i) && self.controls.get(i).zip(next.controls.get(i)).is_some_and(|(a, b)| a.weight != b.weight) {
                    return Err(NurbsError::Control);
                }
            }
        }
        for p in &self.pasted {
            if let Some(q) = next.pasted.iter().find(|q| q.name == p.name) {
                p.curve.validate_corner_weights(&q.curve)?;
            }
        }
        Ok(())
    }
}
