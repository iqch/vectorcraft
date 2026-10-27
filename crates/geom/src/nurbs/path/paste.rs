//! Pasted contours, ported from the user's Matisse NURBS model. A child replaces
//! an interval of the base and its controls follow base tangent/normal frames.
use super::*;
use kurbo::{Affine, Vec2};

const EPS: f64 = 1e-9;
const MAX_PASTES: usize = 128;
const MAX_DEPTH: usize = 8;
const MAX_TREE_CONTROLS: usize = 16384;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum PasteMode {
    #[default]
    #[serde(rename = "u")]
    Normalized,
    #[serde(rename = "p")]
    Parameter,
    #[serde(rename = "l")]
    Length,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PasteAnchor {
    pub coordinate: f64,
    pub tangent: f64,
    pub normal: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NurbsPaste {
    #[serde(default = "default_enabled", skip_serializing_if = "is_enabled")]
    pub enabled: bool,
    pub name: String,
    pub curve: NurbsPath,
    pub mode: PasteMode,
    pub anchors: Vec<PasteAnchor>,
    pub scale: f64,
}

/// A bounded, knot-aware arc-length/projection table. Geometry stays rational;
/// only projection, frames and length binding use numerical estimates.
fn default_enabled() -> bool {
    true
}
fn is_enabled(value: &bool) -> bool {
    *value
}

struct Frames<'a> {
    base: &'a NurbsPath,
    grid: Vec<(f64, Point, f64)>,
    length: f64,
}
impl<'a> Frames<'a> {
    fn new(base: &'a NurbsPath) -> Result<Self, NurbsError> {
        let mut knots = base.knots();
        knots.extend([0.0, 1.0]);
        knots.sort_by(f64::total_cmp);
        knots.dedup();
        let mut grid = vec![(0.0, base.evaluate(0.0)?, 0.0)];
        let mut length = 0.0;
        for span in knots.windows(2) {
            let [a, b] = *span else { continue };
            if b <= a {
                continue;
            }
            for i in 1..=32 {
                if grid.len() >= 131073 {
                    return Err(NurbsError::Size);
                }
                let u = a + (b - a) * i as f64 / 32.0;
                let point = base.evaluate(u)?;
                length += point.distance(grid.last().ok_or(NurbsError::Numerical)?.1);
                grid.push((u, point, length));
            }
        }
        if !length.is_finite() || length <= 1e-12 {
            return Err(NurbsError::Paste);
        }
        Ok(Self { base, grid, length })
    }
    fn coordinate(&self, u: f64, mode: PasteMode) -> f64 {
        if mode != PasteMode::Length {
            return u;
        }
        let turn = u.floor();
        let t = u - turn;
        let k = self.grid.partition_point(|v| v.0 <= t).clamp(1, self.grid.len() - 1);
        let (a, _, la) = self.grid[k - 1];
        let (b, _, lb) = self.grid[k];
        turn * self.length + la + (lb - la) * (t - a) / (b - a)
    }
    fn parameter(&self, coordinate: f64, mode: PasteMode) -> Result<f64, NurbsError> {
        if !coordinate.is_finite() {
            return Err(NurbsError::Paste);
        }
        let u = if mode == PasteMode::Length {
            let turn = (coordinate / self.length).floor();
            let l = coordinate - turn * self.length;
            let k = self.grid.partition_point(|v| v.2 <= l).clamp(1, self.grid.len() - 1);
            let (a, _, la) = self.grid[k - 1];
            let (b, _, lb) = self.grid[k];
            turn + a + (b - a) * if lb > la { (l - la) / (lb - la) } else { 0.0 }
        } else {
            coordinate
        };
        if !u.is_finite() || (!self.base.periodic && !(-EPS..=1.0 + EPS).contains(&u)) {
            return Err(NurbsError::Paste);
        }
        Ok(if self.base.periodic { u } else { u.clamp(0.0, 1.0) })
    }
    fn frame(&self, u: f64) -> Result<(Point, Vec2, Vec2), NurbsError> {
        let t = if self.base.periodic { u.rem_euclid(1.0) } else { u.clamp(0.0, 1.0) };
        let position = self.base.evaluate(t)?;
        for h in [1e-6, 1e-4, 1e-2] {
            let param = |v: f64| if self.base.periodic { v.rem_euclid(1.0) } else { v.clamp(0.0, 1.0) };
            let delta = self.base.evaluate(param(t + h))? - self.base.evaluate(param(t - h))?;
            let length = delta.hypot();
            if length.is_finite() && length > 1e-12 {
                let tangent = delta / length;
                return Ok((position, tangent, Vec2::new(-tangent.y, tangent.x)));
            }
        }
        Err(NurbsError::Paste)
    }
    fn nearest(&self, point: Point) -> Result<f64, NurbsError> {
        let mut best = (f64::INFINITY, 0usize);
        for (i, pair) in self.grid.windows(2).enumerate() {
            let delta = pair[1].1 - pair[0].1;
            let denominator = delta.hypot2();
            let f = if denominator > 0.0 { ((point - pair[0].1).dot(delta) / denominator).clamp(0.0, 1.0) } else { 0.0 };
            let d = point.distance_squared(pair[0].1 + delta * f);
            if d < best.0 {
                best = (d, i);
            }
        }
        let k = best.1;
        let mut a = self.grid[k.saturating_sub(1)].0;
        let mut b = self.grid[(k + 2).min(self.grid.len() - 1)].0;
        for _ in 0..48 {
            let x = a + (b - a) / 3.0;
            let y = b - (b - a) / 3.0;
            if self.base.evaluate(x)?.distance_squared(point) <= self.base.evaluate(y)?.distance_squared(point) {
                b = y;
            } else {
                a = x;
            }
        }
        let mid = (a + b) * 0.5;
        let mut result = mid;
        for u in [0.0, 1.0, self.grid[k].0, self.grid[k + 1].0] {
            if self.base.evaluate(u)?.distance_squared(point) < self.base.evaluate(result)?.distance_squared(point) {
                result = u;
            }
        }
        Ok(result)
    }
    fn interval(&self, pasted: &NurbsPaste) -> Result<(f64, f64), NurbsError> {
        let a = self.parameter(pasted.anchors.first().ok_or(NurbsError::Paste)?.coordinate, pasted.mode)?;
        let b = self.parameter(pasted.anchors.last().ok_or(NurbsError::Paste)?.coordinate, pasted.mode)?;
        let start = a.min(b);
        let end = a.max(b);
        if end - start <= EPS || (self.base.periodic && end - start >= 1.0 - EPS) {
            return Err(NurbsError::Paste);
        }
        Ok((start, end))
    }
    fn materialize(&self, pasted: &NurbsPaste) -> Result<NurbsPath, NurbsError> {
        let mut pieces = pasted.curve.pieces.clone();
        let mut offset = 0;
        for piece in &mut pieces {
            for local in 0..piece.controls().len() {
                let index = offset + local;
                let anchor = pasted.anchors.get(index).ok_or(NurbsError::Paste)?;
                let u = self.parameter(anchor.coordinate, pasted.mode)?;
                let (p, t, n) = self.frame(u)?;
                let original = piece.controls().get(local).ok_or(NurbsError::Index)?;
                *piece =
                    piece.set_control(local, ControlPoint { position: p + (t * anchor.tangent + n * anchor.normal) * pasted.scale, ..*original })?;
            }
            offset += piece.controls().len() - 1;
        }
        NurbsPath::from_parts(pieces, false)?.with_pastes(pasted.curve.pasted.clone())
    }
}

impl NurbsPath {
    /// Intrinsic source at a pasted path; empty path names the root base.
    pub fn source_at(&self, path: &[String]) -> Option<&Self> {
        if path.len() > MAX_DEPTH {
            return None;
        }
        let mut result = self;
        for name in path {
            result = &result.pasted.iter().find(|p| &p.name == name)?.curve;
        }
        Some(result)
    }
    /// Current posed geometry at a path, including ancestor deformations.
    pub fn materialize_at(&self, path: &[String]) -> Result<Self, NurbsError> {
        if path.len() > MAX_DEPTH {
            return Err(NurbsError::Size);
        }
        let mut result = self.clone();
        for name in path {
            result = result.materialize_paste(name)?;
        }
        Ok(result)
    }
    /// Replace a subtree while retaining all ancestor bindings. Changing the child
    /// control count requires replacing its parent's anchors in the full model too.
    pub fn replace_source_at(&self, path: &[String], replacement: Self) -> Result<Self, NurbsError> {
        if path.len() > MAX_DEPTH {
            return Err(NurbsError::Size);
        }
        let Some((name, rest)) = path.split_first() else { return Ok(replacement) };
        let mut pasted = self.pasted.clone();
        let p = pasted.iter_mut().find(|p| &p.name == name).ok_or(NurbsError::Index)?;
        p.curve = p.curve.replace_source_at(rest, replacement)?;
        self.clone().with_pastes(pasted)
    }
    pub fn pastes(&self) -> &[NurbsPaste] {
        &self.pasted
    }
    pub fn with_pastes(mut self, pasted: Vec<NurbsPaste>) -> Result<Self, NurbsError> {
        self.pasted = pasted;
        let mut budget = 0;
        self.validate_pastes(0, &mut budget)?;
        Ok(self)
    }
    pub(super) fn keep_pastes(&self, result: Result<Self, NurbsError>) -> Result<Self, NurbsError> {
        result?.with_pastes(self.pasted.clone())
    }
    fn validate_pastes(&self, depth: usize, budget: &mut usize) -> Result<(), NurbsError> {
        *budget += self.controls.len();
        if depth > MAX_DEPTH || *budget > MAX_TREE_CONTROLS || self.pasted.len() > MAX_PASTES {
            return Err(NurbsError::Size);
        }
        if self.pasted.is_empty() {
            return Ok(());
        }
        let frames = Frames::new(self)?;
        let mut ranges = Vec::new();
        let mut names = std::collections::HashSet::new();
        for p in &self.pasted {
            if p.name.is_empty()
                || p.name.len() > 256
                || !names.insert(&p.name)
                || p.curve.periodic
                || p.anchors.len() != p.curve.controls.len()
                || !p.scale.is_finite()
                || p.scale.abs() > 1e100
            {
                return Err(NurbsError::Paste);
            }
            for anchor in &p.anchors {
                if !anchor.coordinate.is_finite()
                    || anchor.coordinate.abs() > 1e100
                    || !anchor.tangent.is_finite()
                    || anchor.tangent.abs() > 1e100
                    || !anchor.normal.is_finite()
                    || anchor.normal.abs() > 1e100
                {
                    return Err(NurbsError::Paste);
                }
                let _ = frames.parameter(anchor.coordinate, p.mode)?;
            }
            // Endpoints must remain attached to the base exactly.
            if [p.anchors.first(), p.anchors.last()].into_iter().flatten().any(|a| a.tangent != 0.0 || a.normal != 0.0) {
                return Err(NurbsError::Paste);
            }
            p.curve.validate_pastes(depth + 1, budget)?;
            let range = frames.interval(p)?;
            for anchor in &p.anchors {
                let u = frames.parameter(anchor.coordinate, p.mode)?;
                if u < range.0 - EPS || u > range.1 + EPS {
                    return Err(NurbsError::Paste);
                }
            }
            if !p.enabled {
                continue;
            }
            for (a, b) in interval_parts(range, self.periodic) {
                if ranges.iter().any(|&(x, y)| a < y - EPS && b > x + EPS) {
                    return Err(NurbsError::Paste);
                }
                ranges.push((a, b));
            }
        }
        Ok(())
    }
    /// Attach an open child. Project endpoints and store each CV in the base frame.
    pub fn paste_curve(&self, child: Self, name: String, mode: PasteMode, scale: f64) -> Result<Self, NurbsError> {
        if child.periodic || child.controls.len() < 2 {
            return Err(NurbsError::Paste);
        }
        let frames = Frames::new(self)?;
        let first = frames.nearest(child.controls.first().ok_or(NurbsError::Size)?.position)?;
        let mut last = frames.nearest(child.controls.last().ok_or(NurbsError::Size)?.position)?;
        if self.periodic {
            last += (first - last).round();
        }
        let (start, end) = (first.min(last), first.max(last));
        if end - start <= EPS {
            return Err(NurbsError::Paste);
        }
        let middle = (start + end) * 0.5;
        let mut anchors = Vec::new();
        for (i, cp) in child.controls.iter().enumerate() {
            let endpoint = i == 0 || i + 1 == child.controls.len();
            let mut u = if i == 0 {
                first
            } else if endpoint {
                last
            } else {
                frames.nearest(cp.position)?
            };
            if self.periodic && !endpoint {
                u += (middle - u).round();
            }
            let at = u.clamp(start, end);
            let (p, t, n) = frames.frame(at)?;
            let d = cp.position - p;
            anchors.push(PasteAnchor {
                coordinate: frames.coordinate(at, mode),
                tangent: if endpoint || (u >= start && u <= end) { 0.0 } else { d.dot(t) },
                normal: if endpoint { 0.0 } else { d.dot(n) },
            });
        }
        let mut pasted = self.pasted.clone();
        pasted.push(NurbsPaste { name, curve: child, mode, anchors, scale, enabled: true });
        self.clone().with_pastes(pasted)
    }
    /// Ranges refer to the undeformed base, in normalized parameter and local arc length.
    pub fn paste_ranges(&self, name: &str) -> Result<([f64; 2], [f64; 2]), NurbsError> {
        let frames = Frames::new(self)?;
        let p = self.pasted.iter().find(|p| p.name == name).ok_or(NurbsError::Index)?;
        let (a, b) = frames.interval(p)?;
        Ok(([a, b], [frames.coordinate(a, PasteMode::Length), frames.coordinate(b, PasteMode::Length)]))
    }
    pub fn length_range_to_parameters(&self, range: [f64; 2]) -> Result<[f64; 2], NurbsError> {
        let frames = Frames::new(self)?;
        Ok([frames.parameter(range[0], PasteMode::Length)?, frames.parameter(range[1], PasteMode::Length)?])
    }
    pub fn materialize_paste(&self, name: &str) -> Result<Self, NurbsError> {
        let p = self.pasted.iter().find(|p| p.name == name).ok_or(NurbsError::Index)?;
        Frames::new(self)?.materialize(p)
    }
    /// Move/resize a direct child. Range is normalized base parameter; orientation is retained.
    pub fn update_paste(&self, name: &str, range: Option<[f64; 2]>, scale: Option<f64>) -> Result<Self, NurbsError> {
        let frames = Frames::new(self)?;
        let mut pasted = self.pasted.clone();
        let child = pasted.iter_mut().find(|p| p.name == name).ok_or(NurbsError::Index)?;
        if let Some([a, b]) = range {
            if !a.is_finite() || !b.is_finite() || b <= a || (self.periodic && b - a >= 1.0 - EPS) {
                return Err(NurbsError::Paste);
            }
            let first = frames.parameter(child.anchors.first().ok_or(NurbsError::Paste)?.coordinate, child.mode)?;
            let last = frames.parameter(child.anchors.last().ok_or(NurbsError::Paste)?.coordinate, child.mode)?;
            let (new_first, new_last) = if last >= first { (a, b) } else { (b, a) };
            for anchor in &mut child.anchors {
                let old = frames.parameter(anchor.coordinate, child.mode)?;
                let u = new_first + (old - first) / (last - first) * (new_last - new_first);
                anchor.coordinate = frames.coordinate(u, child.mode);
            }
        }
        if let Some(value) = scale {
            child.scale = value;
        }
        self.clone().with_pastes(pasted)
    }
    pub fn set_paste_enabled(&self, name: &str, enabled: bool) -> Result<Self, NurbsError> {
        let mut pasted = self.pasted.clone();
        let p = pasted.iter_mut().find(|p| p.name == name).ok_or(NurbsError::Index)?;
        p.enabled = enabled;
        self.clone().with_pastes(pasted)
    }
    /// Copies initially stay disabled at the same range, so active intervals do not overlap.
    pub fn duplicate_paste(&self, name: &str, new_name: Option<&str>) -> Result<(Self, String), NurbsError> {
        let mut copy = self.pasted.iter().find(|p| p.name == name).ok_or(NurbsError::Index)?.clone();
        let resolved = if let Some(value) = new_name {
            value.to_string()
        } else {
            let mut number = 2usize;
            loop {
                let suffix = format!(" ({number})");
                let mut stem = name.to_string();
                while stem.len() + suffix.len() > 256 {
                    stem.pop();
                }
                let candidate = stem + &suffix;
                if !self.pasted.iter().any(|p| p.name == candidate) {
                    break candidate;
                }
                number += 1;
            }
        };
        copy.name = resolved.clone();
        copy.enabled = false;
        let mut pasted = self.pasted.clone();
        pasted.push(copy);
        Ok((self.clone().with_pastes(pasted)?, resolved))
    }
    pub fn remove_paste(&self, name: &str) -> Result<Self, NurbsError> {
        if !self.pasted.iter().any(|p| p.name == name) {
            return Err(NurbsError::Index);
        }
        self.clone().with_pastes(self.pasted.iter().filter(|p| p.name != name).cloned().collect())
    }
    pub fn transformed(&self, xf: Affine) -> Result<Self, NurbsError> {
        if xf == Affine::IDENTITY {
            return Ok(self.clone());
        }
        let mut pieces = self.pieces.clone();
        for piece in &mut pieces {
            for i in 0..piece.controls().len() {
                let c = *piece.controls().get(i).ok_or(NurbsError::Index)?;
                *piece = piece.set_control(i, ControlPoint { position: xf * c.position, ..c })?;
            }
        }
        let result = Self::from_parts(pieces, self.periodic)?.ordered(self.control_start, self.parameter_start)?;
        if self.pasted.is_empty() {
            return Ok(result);
        }
        let old_frames = Frames::new(self)?;
        let new_frames = Frames::new(&result)?;
        let mut pasted = Vec::new();
        for p in &self.pasted {
            let mut child = p.clone();
            child.curve = p.curve.transformed(xf)?;
            for anchor in &mut child.anchors {
                let u = old_frames.parameter(anchor.coordinate, p.mode)?;
                let (_, old_t, old_n) = old_frames.frame(u)?;
                let (_, new_t, new_n) = new_frames.frame(u)?;
                let offset = old_t * anchor.tangent + old_n * anchor.normal;
                let delta = xf * (Point::ORIGIN + offset) - xf * Point::ORIGIN;
                anchor.coordinate = new_frames.coordinate(u, p.mode);
                anchor.tangent = delta.dot(new_t);
                anchor.normal = delta.dot(new_n);
            }
            pasted.push(child);
        }
        result.with_pastes(pasted)
    }
    /// Evaluate a bounded sample batch, preparing pasted frames once per subtree.
    pub fn colored_samples(&self, parameters: &[f64], hsv: bool) -> Result<Vec<(Point, [f64; 4])>, NurbsError> {
        if parameters.len() > 32769 {
            return Err(NurbsError::Size);
        }
        let mut out = parameters.iter().map(|&u| Ok((self.evaluate(u)?, self.evaluate_color(u, hsv)?))).collect::<Result<Vec<_>, NurbsError>>()?;
        if self.pasted.iter().all(|p| !p.enabled) {
            return Ok(out);
        }
        let frames = Frames::new(self)?;
        for p in self.pasted.iter().filter(|p| p.enabled) {
            let (a, b) = frames.interval(p)?;
            let first = frames.parameter(p.anchors.first().ok_or(NurbsError::Paste)?.coordinate, p.mode)?;
            let mut selected = Vec::new();
            let mut local = Vec::new();
            for (i, &u) in parameters.iter().enumerate() {
                let t = if self.periodic { u + (a - u).ceil() } else { u };
                if t >= a - EPS && t <= b + EPS {
                    let v = ((t - a) / (b - a)).clamp(0.0, 1.0);
                    selected.push(i);
                    local.push(if first <= a + EPS { v } else { 1.0 - v });
                }
            }
            let child = frames.materialize(p)?;
            for (i, value) in selected.into_iter().zip(child.colored_samples(&local, hsv)?) {
                *out.get_mut(i).ok_or(NurbsError::Index)? = value;
            }
        }
        Ok(out)
    }
    /// Composite evaluation; `evaluate` deliberately remains the undeformed base.
    pub fn evaluate_composed(&self, u: f64) -> Result<Point, NurbsError> {
        if !u.is_finite() || !(0.0..=1.0).contains(&u) {
            return Err(NurbsError::Parameter);
        }
        if self.pasted.is_empty() {
            return self.evaluate(u);
        }
        let frames = Frames::new(self)?;
        for p in self.pasted.iter().filter(|p| p.enabled) {
            let (a, b) = frames.interval(p)?;
            let t = if self.periodic { u + (a - u).ceil() } else { u };
            if t >= a - EPS && t <= b + EPS {
                let first = frames.parameter(p.anchors.first().ok_or(NurbsError::Paste)?.coordinate, p.mode)?;
                let local = ((t - a) / (b - a)).clamp(0.0, 1.0);
                return frames.materialize(p)?.evaluate_composed(if first <= a + EPS { local } else { 1.0 - local });
            }
        }
        self.evaluate(u)
    }
    pub(super) fn composed_bezpath(&self, tolerance: f64) -> Result<BezPath, NurbsError> {
        let frames = Frames::new(self)?;
        let mut parts = Vec::new();
        for p in self.pasted.iter().filter(|p| p.enabled) {
            let (a, b) = frames.interval(p)?;
            let first = frames.parameter(p.anchors.first().ok_or(NurbsError::Paste)?.coordinate, p.mode)?;
            let child = frames.materialize(p)?;
            for (start, end) in interval_parts((a, b), self.periodic) {
                let shift = if self.periodic { (a - start).ceil() } else { 0.0 };
                let x = (start + shift - a) / (b - a);
                let y = (end + shift - a) / (b - a);
                parts.push((start, end, child.clone(), if first <= a + EPS { x } else { 1.0 - x }, if first <= a + EPS { y } else { 1.0 - y }));
            }
        }
        parts.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut out = BezPath::new();
        let mut cursor = 0.0;
        for (start, end, child, x, y) in parts {
            if start > cursor {
                append_path(&mut out, self.base_range(cursor, start, tolerance)?)?;
            }
            append_path(&mut out, child.composed_range(x.clamp(0.0, 1.0), y.clamp(0.0, 1.0), tolerance)?)?;
            cursor = end;
        }
        if cursor < 1.0 {
            append_path(&mut out, self.base_range(cursor, 1.0, tolerance)?)?;
        }
        if self.periodic {
            out.close_path();
        }
        Ok(out)
    }
    fn composed_range(&self, a: f64, b: f64, tolerance: f64) -> Result<BezPath, NurbsError> {
        if self.pasted.is_empty() {
            return self.base_range(a, b, tolerance);
        }
        // Split in parameter space, preserving exact rational pieces of nested children.
        let frames = Frames::new(self)?;
        let reverse = b < a;
        let (lo, hi) = (a.min(b), a.max(b));
        let mut cuts = vec![lo, hi];
        for p in self.pasted.iter().filter(|p| p.enabled) {
            let (x, y) = frames.interval(p)?;
            for t in [x, y] {
                if t > lo && t < hi {
                    cuts.push(t);
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup();
        let mut out = BezPath::new();
        for pair in cuts.windows(2) {
            let x = pair[0];
            let y = pair[1];
            let mid = (x + y) * 0.5;
            let mut found = false;
            for p in self.pasted.iter().filter(|p| p.enabled) {
                let (start, end) = frames.interval(p)?;
                if mid > start && mid < end {
                    let first = frames.parameter(p.anchors.first().ok_or(NurbsError::Paste)?.coordinate, p.mode)?;
                    let (u, v) = ((x - start) / (end - start), (y - start) / (end - start));
                    let (u, v) = if first <= start + EPS { (u, v) } else { (1.0 - u, 1.0 - v) };
                    append_path(&mut out, frames.materialize(p)?.composed_range(u, v, tolerance)?)?;
                    found = true;
                    break;
                }
            }
            if !found {
                append_path(&mut out, self.base_range(x, y, tolerance)?)?;
            }
        }
        Ok(if reverse { out.reverse_subpaths() } else { out })
    }
    fn base_range(&self, a: f64, b: f64, tolerance: f64) -> Result<BezPath, NurbsError> {
        let reverse = b < a;
        let (a, b) = (a.min(b), a.max(b));
        let count = self.pieces.len() as f64;
        let mut out = BezPath::new();
        for (i, source) in self.pieces.iter().enumerate() {
            let start = (a * count - i as f64).max(0.0);
            let end = (b * count - i as f64).min(1.0);
            if end <= start {
                continue;
            }
            let mut piece = source.as_clamped()?;
            if end < 1.0 {
                piece = piece.split(end)?.0;
            }
            if start > 0.0 {
                piece = piece.split(start / end)?.1;
            }
            append_path(&mut out, piece.to_bezpath(tolerance)?)?;
        }
        Ok(if reverse { out.reverse_subpaths() } else { out })
    }
}
fn interval_parts((a, b): (f64, f64), periodic: bool) -> Vec<(f64, f64)> {
    if !periodic {
        return vec![(a, b)];
    }
    let start = a.rem_euclid(1.0);
    let end = start + b - a;
    if end <= 1.0 { vec![(start, end)] } else { vec![(start, 1.0), (0.0, end - 1.0)] }
}
fn append_path(out: &mut BezPath, path: BezPath) -> Result<(), NurbsError> {
    for el in path.elements() {
        if matches!(el, PathEl::ClosePath) || (!out.elements().is_empty() && matches!(el, PathEl::MoveTo(_))) {
            continue;
        }
        if out.elements().len() >= 32768 {
            return Err(NurbsError::Size);
        }
        out.push(*el);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::ParamCurveNearest;
    fn path(points: &[(f64, f64)]) -> NurbsPath {
        NurbsPath::from_controls(points.iter().map(|&(x, y)| ControlPoint::new(Point::new(x, y), 1.0)).collect()).unwrap()
    }
    fn base() -> NurbsPath {
        path(&[(0.0, 0.0), (100.0, 0.0)])
    }
    fn child() -> NurbsPath {
        path(&[(20.0, 0.0), (30.0, 20.0), (40.0, 0.0)])
    }
    fn close(a: Point, b: Point) {
        assert!(a.distance(b) < 1e-6, "{a:?} != {b:?}");
    }
    fn check_outline(c: &NurbsPath) {
        let outline = c.to_bezpath(0.001).unwrap();
        for i in 0..=180 {
            let point = c.evaluate_composed(i as f64 / 180.0).unwrap();
            let d = outline.segments().map(|s| s.nearest(point, 1e-9).distance_sq).fold(f64::INFINITY, f64::min);
            assert!(d.sqrt() < 0.002, "outline misses {point:?}: {}", d.sqrt());
        }
    }
    #[test]
    fn pasted_replaces_interval_and_tracks_parent_frames() {
        let c = base().paste_curve(child(), "bump".into(), PasteMode::Normalized, 1.0).unwrap();
        close(c.evaluate_composed(0.1).unwrap(), Point::new(10.0, 0.0));
        close(c.evaluate_composed(0.3).unwrap(), Point::new(30.0, 10.0));
        close(c.evaluate_composed(0.5).unwrap(), Point::new(50.0, 0.0));
        check_outline(&c);
        let moved = c.set_control(1, ControlPoint::new(Point::new(100.0, 100.0), 1.0)).unwrap();
        let d = 10.0 / 2.0f64.sqrt();
        close(moved.evaluate_composed(0.3).unwrap(), Point::new(30.0 - d, 30.0 + d));
        assert_eq!(moved.pastes().len(), 1);
        check_outline(&moved);
        assert_eq!(c.remove_paste("bump").unwrap(), base());
    }
    #[test]
    fn pasted_move_resize_scale_modes_and_reverse_direction() {
        for mode in [PasteMode::Normalized, PasteMode::Parameter, PasteMode::Length] {
            let c = base().paste_curve(child(), "bump".into(), mode, 1.0).unwrap();
            let moved = c.update_paste("bump", Some([0.5, 0.9]), Some(-2.0)).unwrap();
            close(moved.evaluate_composed(0.7).unwrap(), Point::new(70.0, -20.0));
            let (u, l) = moved.paste_ranges("bump").unwrap();
            assert!((u[0] - 0.5).abs() < 1e-7 && (l[1] - 90.0).abs() < 1e-6);
            assert_eq!(moved.length_range_to_parameters([50.0, 90.0]).unwrap(), [0.5, 0.9]);
            check_outline(&moved);
        }
        let reverse = path(&[(40.0, 0.0), (30.0, 20.0), (20.0, 0.0)]);
        let c = base().paste_curve(reverse, "reverse".into(), PasteMode::Normalized, 1.0).unwrap();
        close(c.evaluate_composed(0.3).unwrap(), Point::new(30.0, 10.0));
        check_outline(&c);
        let full = base().paste_curve(path(&[(0.0, 0.0), (50.0, 20.0), (100.0, 0.0)]), "full".into(), PasteMode::Normalized, 1.0).unwrap();
        check_outline(&full);
    }
    #[test]
    fn pasted_wire_validation_overlap_and_editor_preservation() {
        let c = base().paste_curve(child(), "bump".into(), PasteMode::Normalized, 1.0).unwrap();
        let wire = serde_json::to_value(&c).unwrap();
        assert_eq!(c, serde_json::from_value::<NurbsPath>(wire.clone()).unwrap());
        let mut bad = wire.clone();
        bad["pasted"][0]["anchors"][0]["normal"] = serde_json::json!(1.0);
        assert!(serde_json::from_value::<NurbsPath>(bad).is_err());
        let mut bad = wire.clone();
        bad["pasted"][0]["anchors"] = serde_json::json!([]);
        assert!(serde_json::from_value::<NurbsPath>(bad).is_err());
        let mut bad = wire.clone();
        bad["pasted"].as_array_mut().unwrap().push(wire["pasted"][0].clone());
        assert!(serde_json::from_value::<NurbsPath>(bad).is_err());
        assert!(c.paste_curve(child(), "overlap".into(), PasteMode::Normalized, 1.0).is_err());
        assert!(c.paste_curve(child(), "bump".into(), PasteMode::Normalized, 1.0).is_err());
        assert!(c.update_paste("bump", Some([-0.1, 0.3]), None).is_err());
        assert!(c.update_paste("bump", None, Some(f64::NAN)).is_err());
        assert!(c.remove_paste("missing").is_err());
        let refined = c.insert_vertex(0.6).unwrap();
        assert_eq!(refined.pastes(), c.pastes());
        for i in 0..=30 {
            close(c.evaluate_composed(i as f64 / 30.0).unwrap(), refined.evaluate_composed(i as f64 / 30.0).unwrap());
        }
    }
    #[test]
    fn pasted_periodic_seam_and_opening_rejection() {
        let c = NurbsPath::with_topology(
            vec![
                ControlPoint::new(Point::new(0.0, 0.0), 1.0),
                ControlPoint::new(Point::new(100.0, 0.0), 1.0),
                ControlPoint::new(Point::new(100.0, 100.0), 1.0),
                ControlPoint::new(Point::new(0.0, 100.0), 1.0),
            ],
            vec![],
            true,
        )
        .unwrap();
        let a = c.evaluate(0.9).unwrap();
        let b = c.evaluate(0.1).unwrap();
        let curve = path(&[(a.x, a.y), (50.0, -30.0), (b.x, b.y)]);
        let attached = c.paste_curve(curve, "seam".into(), PasteMode::Normalized, 1.0).unwrap();
        assert!(attached.paste_ranges("seam").unwrap().0[1] > 1.0);
        close(attached.evaluate_composed(0.0).unwrap(), attached.evaluate_composed(1.0).unwrap());
        check_outline(&attached);
        assert!(attached.with_periodic(false).is_err());
        let restored: NurbsPath = serde_json::from_value(serde_json::to_value(&attached).unwrap()).unwrap();
        assert_eq!(restored, attached);
    }
    #[test]
    fn pasted_nested_composition_and_transform() {
        let child_base = path(&[(20.0, 0.0), (40.0, 0.0)]);
        let inner = path(&[(25.0, 0.0), (30.0, 8.0), (35.0, 0.0)]);
        let nested = child_base.paste_curve(inner, "inner".into(), PasteMode::Normalized, 1.0).unwrap();
        let c = base().paste_curve(nested, "outer".into(), PasteMode::Normalized, 1.0).unwrap();
        close(c.evaluate_composed(0.3).unwrap(), Point::new(30.0, 4.0));
        check_outline(&c);
        let xf = Affine::new([2.0, 0.3, 0.4, 1.5, 7.0, 13.0]);
        let transformed = c.transformed(xf).unwrap();
        for i in 0..=30 {
            close(transformed.evaluate_composed(i as f64 / 30.0).unwrap(), xf * c.evaluate_composed(i as f64 / 30.0).unwrap());
        }
        check_outline(&transformed);
    }
    #[test]
    fn pasted_rational_parent_matches_independent_circle_frames() {
        let c = NurbsPath::from_controls(vec![
            ControlPoint::new(Point::new(100.0, 0.0), 1.0),
            ControlPoint::new(Point::new(100.0, 100.0), 0.5f64.sqrt()),
            ControlPoint::new(Point::new(0.0, 100.0), 1.0),
        ])
        .unwrap();
        let p0 = c.evaluate(0.2).unwrap();
        let p1 = c.evaluate(0.35).unwrap();
        let p2 = c.evaluate(0.6).unwrap();
        let tangent = Vec2::new(-p1.y, p1.x) / 100.0;
        let normal = Vec2::new(-tangent.y, tangent.x);
        let q1 = p1 + tangent * 3.0 + normal * 12.0;
        let attached = c
            .clone()
            .with_pastes(vec![NurbsPaste {
                name: "circle".into(),
                enabled: true,
                curve: child(),
                mode: PasteMode::Normalized,
                scale: 1.0,
                anchors: vec![
                    PasteAnchor { coordinate: 0.2, tangent: 0.0, normal: 0.0 },
                    PasteAnchor { coordinate: 0.35, tangent: 3.0, normal: 12.0 },
                    PasteAnchor { coordinate: 0.6, tangent: 0.0, normal: 0.0 },
                ],
            }])
            .unwrap();
        let expected = Point::new((p0.x + 2.0 * q1.x + p2.x) / 4.0, (p0.y + 2.0 * q1.y + p2.y) / 4.0);
        close(attached.evaluate_composed(0.4).unwrap(), expected);
        check_outline(&attached);
        let shifted = attached.update_paste("circle", Some([0.5, 0.9]), None).unwrap();
        check_outline(&shifted);
    }
    #[test]
    fn pasted_rejects_excessive_nesting_and_periodic_child() {
        let periodic = NurbsPath::with_topology(child().controls().to_vec(), vec![], true).unwrap();
        assert!(base().paste_curve(periodic, "loop".into(), PasteMode::Normalized, 1.0).is_err());
        let mut nested = base();
        for i in 0..8 {
            nested = base().paste_curve(nested, format!("depth-{i}"), PasteMode::Normalized, 1.0).unwrap();
        }
        assert!(base().paste_curve(nested, "too-deep".into(), PasteMode::Normalized, 1.0).is_err());
    }
    #[test]
    fn pasted_disable_duplicate_enable_and_legacy_wire() {
        let c = base().paste_curve(child(), "Petal".into(), PasteMode::Normalized, 1.0).unwrap();
        let disabled = c.set_paste_enabled("Petal", false).unwrap();
        close(disabled.evaluate_composed(0.3).unwrap(), Point::new(30.0, 0.0));
        check_outline(&disabled);
        let wire = serde_json::to_value(&disabled).unwrap();
        assert_eq!(wire["pasted"][0]["enabled"], serde_json::json!(false));
        assert_eq!(disabled, serde_json::from_value::<NurbsPath>(wire).unwrap());
        let legacy = serde_json::to_value(&c).unwrap();
        assert!(legacy["pasted"][0].get("enabled").is_none());
        assert!(serde_json::from_value::<NurbsPath>(legacy).unwrap().pastes()[0].enabled);
        let (copy, name) = c.duplicate_paste("Petal", None).unwrap();
        assert_eq!(name, "Petal (2)");
        assert!(!copy.pastes()[1].enabled);
        close(copy.evaluate_composed(0.3).unwrap(), c.evaluate_composed(0.3).unwrap());
        assert!(copy.set_paste_enabled(&name, true).is_err());
        let moved = copy.update_paste(&name, Some([0.6, 0.8]), None).unwrap().set_paste_enabled(&name, true).unwrap();
        close(moved.evaluate_composed(0.7).unwrap(), Point::new(70.0, 10.0));
        check_outline(&moved);
        let (third, name) = moved.duplicate_paste("Petal", None).unwrap();
        assert_eq!(name, "Petal (3)");
        assert_eq!(third.pastes().len(), 3);
        assert!(c.duplicate_paste("Petal", Some("Petal")).is_err());
    }
}
