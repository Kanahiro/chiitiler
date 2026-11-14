//! Symbol anchor geometry: line clipping, anchors along lines (`get_anchors.ts`,
//! `check_max_angle.ts`, `clip_line.ts`), line merging and the pole of inaccessibility.

use crate::expression::value::js_round;

pub type P = [f64; 2];

#[derive(Debug, Clone, Copy)]
pub struct Anchor {
    pub x: f64,
    pub y: f64,
    /// Index of the line segment the anchor is on (line placement).
    pub segment: Option<usize>,
}

pub fn dist(a: P, b: P) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// `Point.angleTo`.
fn angle_to(a: P, b: P) -> f64 {
    (a[1] - b[1]).atan2(a[0] - b[0])
}

fn round(p: P) -> P {
    [js_round(p[0]), js_round(p[1])]
}

/// `clipLine`: clip lines to `[x1, x2) x [y1, y2)`, rounding the cut points.
pub fn clip_line(lines: &[Vec<P>], x1: f64, y1: f64, x2: f64, y2: f64) -> Vec<Vec<P>> {
    let mut out: Vec<Vec<P>> = Vec::new();
    for line in lines {
        let mut current: Option<usize> = None;
        for i in 0..line.len().saturating_sub(1) {
            let (mut p0, mut p1) = (line[i], line[i + 1]);
            let at_x = |p0: P, p1: P, x: f64| round([x, p0[1] + (p1[1] - p0[1]) * ((x - p0[0]) / (p1[0] - p0[0]))]);
            let at_y = |p0: P, p1: P, y: f64| round([p0[0] + (p1[0] - p0[0]) * ((y - p0[1]) / (p1[1] - p0[1])), y]);
            if p0[0] < x1 && p1[0] < x1 {
                continue;
            } else if p0[0] < x1 {
                p0 = at_x(p0, p1, x1);
            } else if p1[0] < x1 {
                p1 = at_x(p0, p1, x1);
            }
            if p0[1] < y1 && p1[1] < y1 {
                continue;
            } else if p0[1] < y1 {
                p0 = at_y(p0, p1, y1);
            } else if p1[1] < y1 {
                p1 = at_y(p0, p1, y1);
            }
            if p0[0] >= x2 && p1[0] >= x2 {
                continue;
            } else if p0[0] >= x2 {
                p0 = at_x(p0, p1, x2);
            } else if p1[0] >= x2 {
                p1 = at_x(p0, p1, x2);
            }
            if p0[1] >= y2 && p1[1] >= y2 {
                continue;
            } else if p0[1] >= y2 {
                p0 = at_y(p0, p1, y2);
            } else if p1[1] >= y2 {
                p1 = at_y(p0, p1, y2);
            }
            if current.is_none_or(|c| *out[c].last().unwrap() != p0) {
                out.push(vec![p0]);
                current = Some(out.len() - 1);
            }
            out[current.unwrap()].push(p1);
        }
    }
    out
}

fn line_length(line: &[P]) -> f64 {
    line.windows(2).map(|w| dist(w[0], w[1])).sum()
}

/// `checkMaxAngle`: whether the label fits along the line without bending too sharply.
pub fn check_max_angle(line: &[P], anchor: &Anchor, label_length: f64, window_size: f64, max_angle: f64) -> bool {
    let Some(segment) = anchor.segment else { return true };
    if label_length == 0.0 {
        return true;
    }
    let mut p = [anchor.x, anchor.y];
    let mut index = segment as isize + 1;
    let mut anchor_distance = 0.0;
    while anchor_distance > -label_length / 2.0 {
        index -= 1;
        if index < 0 {
            return false;
        }
        anchor_distance -= dist(line[index as usize], p);
        p = line[index as usize];
    }
    let mut index = index as usize;
    anchor_distance += dist(line[index], line[index + 1]);
    index += 1;
    let mut recent: std::collections::VecDeque<(f64, f64)> = Default::default();
    let mut recent_delta = 0.0;
    while anchor_distance < label_length / 2.0 {
        let (prev, current) = (line[index - 1], line[index]);
        let Some(&next) = line.get(index + 1) else { return false };
        let delta = angle_to(prev, current) - angle_to(current, next);
        let delta = (((delta + 3.0 * std::f64::consts::PI) % (std::f64::consts::PI * 2.0)) - std::f64::consts::PI).abs();
        recent.push_back((anchor_distance, delta));
        recent_delta += delta;
        while anchor_distance - recent[0].0 > window_size {
            recent_delta -= recent.pop_front().unwrap().1;
        }
        if recent_delta > max_angle {
            return false;
        }
        index += 1;
        anchor_distance += dist(current, next);
    }
    true
}

/// `getCenterAnchor` (`symbol-placement: line-center`).
pub fn center_anchor(line: &[P], max_angle: f64, angle_window_size: f64, label_length: f64) -> Option<Anchor> {
    let center = line_length(line) / 2.0;
    let mut prev = 0.0;
    for i in 0..line.len().saturating_sub(1) {
        let (a, b) = (line[i], line[i + 1]);
        let d = dist(a, b);
        if prev + d > center {
            let t = (center - prev) / d;
            let p = round([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
            let anchor = Anchor { x: p[0], y: p[1], segment: Some(i) };
            return (angle_window_size == 0.0 || check_max_angle(line, &anchor, label_length, angle_window_size, max_angle)).then_some(anchor);
        }
        prev += d;
    }
    None
}

/// `getAnchors` (`symbol-placement: line`).
#[allow(clippy::too_many_arguments)]
pub fn line_anchors(line: &[P], spacing: f64, max_angle: f64, angle_window_size: f64, shaped_label_length: f64, box_scale: f64, overscaling: f64, extent: f64) -> Vec<Anchor> {
    let label_length = shaped_label_length * box_scale;
    let continued = line[0][0] == 0.0 || line[0][0] == extent || line[0][1] == 0.0 || line[0][1] == extent;
    let mut spacing = spacing;
    if spacing - label_length < spacing / 4.0 {
        spacing = label_length + spacing / 4.0;
    }
    let fixed_extra_offset = 24.0 * 2.0;
    let offset = if !continued {
        ((shaped_label_length / 2.0 + fixed_extra_offset) * box_scale * overscaling) % spacing
    } else {
        (spacing / 2.0 * overscaling) % spacing
    };
    resample(line, offset, spacing, angle_window_size, max_angle, label_length, continued, false, extent)
}

#[allow(clippy::too_many_arguments)]
fn resample(line: &[P], offset: f64, spacing: f64, window: f64, max_angle: f64, label_length: f64, continued: bool, middle: bool, extent: f64) -> Vec<Anchor> {
    let half = label_length / 2.0;
    let length = line_length(line);
    let mut distance = 0.0;
    let mut marked = offset - spacing;
    let mut anchors = Vec::new();
    for i in 0..line.len().saturating_sub(1) {
        let (a, b) = (line[i], line[i + 1]);
        let d = dist(a, b);
        while marked + spacing < distance + d {
            marked += spacing;
            let t = (marked - distance) / d;
            let (x, y) = (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t);
            if x >= 0.0 && x < extent && y >= 0.0 && y < extent && marked - half >= 0.0 && marked + half <= length {
                let p = round([x, y]);
                let anchor = Anchor { x: p[0], y: p[1], segment: Some(i) };
                if window == 0.0 || check_max_angle(line, &anchor, label_length, window, max_angle) {
                    anchors.push(anchor);
                }
            }
        }
        distance += d;
    }
    if !middle && anchors.is_empty() && !continued {
        anchors = resample(line, distance / 2.0, spacing, window, max_angle, label_length, continued, true, extent);
    }
    anchors
}

/// `mergeLines`: join lines with the same text that share endpoints. Takes `(text, line)` per
/// feature and returns, per feature, its merged line (`None` = merged into another).
pub fn merge_lines(items: &[(Option<String>, Vec<P>)]) -> Vec<Option<Vec<P>>> {
    use crate::hash::HashMap;
    let key = |text: &str, p: P| format!("{text}:{}:{}", p[0], p[1]);
    // Each merged line: owning feature, chunks (feature indices) in order.
    let mut merged: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut alive: Vec<bool> = Vec::new();
    let (mut left, mut right): (HashMap<String, usize>, HashMap<String, usize>) = Default::default();
    let first_pt = |m: &(usize, Vec<usize>)| items[m.1[0]].1[0];
    let last_pt = |m: &(usize, Vec<usize>)| *items[*m.1.last().unwrap()].1.last().unwrap();
    for (fi, (text, line)) in items.iter().enumerate() {
        let Some(text) = text.as_deref().filter(|t| !t.is_empty() && !line.is_empty()) else {
            merged.push((fi, vec![fi]));
            alive.push(true);
            continue;
        };
        let (lk, rk) = (key(text, line[0]), key(text, *line.last().unwrap()));
        match (right.get(&lk).copied(), left.get(&rk).copied()) {
            (Some(ri), Some(li)) if ri != li => {
                // mergeFromLeft: this line goes in front of `li`.
                left.remove(&rk);
                left.insert(lk.clone(), li);
                merged[li].1.insert(0, fi);
                // mergeFromRight: `li` (now starting with this line) goes after `ri`.
                right.remove(&lk);
                right.insert(rk.clone(), ri);
                let chunks = std::mem::take(&mut merged[li].1);
                merged[ri].1.extend(chunks);
                left.remove(&lk);
                right.remove(&rk);
                let end = key(text, last_pt(&merged[ri]));
                right.insert(end, ri);
                alive[li] = false;
            }
            (Some(ri), _) => {
                right.remove(&lk);
                right.insert(rk, ri);
                merged[ri].1.push(fi);
            }
            (None, Some(li)) => {
                left.remove(&rk);
                left.insert(lk, li);
                merged[li].1.insert(0, fi);
            }
            _ => {
                merged.push((fi, vec![fi]));
                alive.push(true);
                left.insert(lk, merged.len() - 1);
                right.insert(rk, merged.len() - 1);
            }
        }
        let _ = first_pt;
    }
    let mut out = vec![None; items.len()];
    for (m, ok) in merged.iter().zip(alive) {
        if !ok {
            continue;
        }
        let mut pts = items[m.1[0]].1.clone();
        for &c in &m.1[1..] {
            pts.extend_from_slice(&items[c].1[1..]);
        }
        out[m.0] = Some(pts);
    }
    out
}

fn dist_to_segment_sq(p: P, a: P, b: P) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let mut q = a;
    if dx != 0.0 || dy != 0.0 {
        let t = ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy);
        if t > 1.0 {
            q = b;
        } else if t > 0.0 {
            q = [a[0] + dx * t, a[1] + dy * t];
        }
    }
    (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)
}

fn point_to_polygon_dist(p: P, polygon: &[Vec<P>]) -> f64 {
    let mut inside = false;
    let mut min = f64::INFINITY;
    for ring in polygon {
        let n = ring.len();
        let mut j = n.wrapping_sub(1);
        for i in 0..n {
            let (a, b) = (ring[i], ring[j]);
            if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
                inside = !inside;
            }
            min = min.min(dist_to_segment_sq(p, a, b));
            j = i;
        }
    }
    (if inside { 1.0 } else { -1.0 }) * min.sqrt()
}

/// `findPoleOfInaccessibility` (polylabel).
pub fn pole_of_inaccessibility(polygon: &[Vec<P>], precision: f64) -> P {
    struct Cell {
        p: P,
        h: f64,
        d: f64,
        max: f64,
    }
    let cell = |x: f64, y: f64, h: f64| {
        let d = point_to_polygon_dist([x, y], polygon);
        Cell { p: [x, y], h, d, max: d + h * std::f64::consts::SQRT_2 }
    };
    let ring = &polygon[0];
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in ring {
        min_x = min_x.min(p[0]);
        min_y = min_y.min(p[1]);
        max_x = max_x.max(p[0]);
        max_y = max_y.max(p[1]);
    }
    let size = (max_x - min_x).min(max_y - min_y);
    if size == 0.0 {
        return [min_x, min_y];
    }
    let h = size / 2.0;
    // ponytail: linear-scan max queue; a binary heap if polygons get huge.
    let mut queue: Vec<Cell> = Vec::new();
    let mut x = min_x;
    while x < max_x {
        let mut y = min_y;
        while y < max_y {
            queue.push(cell(x + h, y + h, h));
            y += size;
        }
        x += size;
    }
    // getCentroidCell
    let (mut area, mut cx, mut cy) = (0.0, 0.0, 0.0);
    let n = ring.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (ring[i], ring[j]);
        let f = a[0] * b[1] - b[0] * a[1];
        cx += (a[0] + b[0]) * f;
        cy += (a[1] + b[1]) * f;
        area += f * 3.0;
        j = i;
    }
    let centroid = cell(cx / area, cy / area, 0.0);
    let mut best = Cell { ..centroid };
    let centroid_d = best.d;
    let centroid_p = best.p;
    while !queue.is_empty() {
        // tinyqueue pops the max `max` (ties: heap order; first found here).
        let i = (0..queue.len()).fold(0, |bi, i| if queue[i].max > queue[bi].max { i } else { bi });
        let c = queue.swap_remove(i);
        if c.d > best.d || best.d == 0.0 || best.d.is_nan() {
            best = Cell { ..c };
        }
        if c.max - best.d <= precision {
            continue;
        }
        let h = c.h / 2.0;
        queue.push(cell(c.p[0] - h, c.p[1] - h, h));
        queue.push(cell(c.p[0] + h, c.p[1] - h, h));
        queue.push(cell(c.p[0] - h, c.p[1] + h, h));
        queue.push(cell(c.p[0] + h, c.p[1] + h, h));
    }
    if centroid_d > 0.0 && best.d - centroid_d <= precision {
        return centroid_p;
    }
    best.p
}

/// `PathInterpolator`.
pub struct PathInterpolator<'a> {
    points: &'a [P],
    distances: Vec<f64>,
    pub length: f64,
    pub padded_length: f64,
    padding: f64,
}

impl<'a> PathInterpolator<'a> {
    pub fn new(points: &'a [P], padding: f64) -> Self {
        let mut distances = vec![0.0];
        for i in 1..points.len() {
            distances.push(distances[i - 1] + dist(points[i], points[i - 1]));
        }
        let length = *distances.last().unwrap();
        let padding = padding.min(length * 0.5);
        PathInterpolator { points, distances, length, padded_length: length - padding * 2.0, padding }
    }

    pub fn lerp(&self, t: f64) -> P {
        if self.points.len() == 1 {
            return self.points[0];
        }
        let t = t.clamp(0.0, 1.0);
        let mut i = 1;
        let target = t * self.padded_length + self.padding;
        while i < self.distances.len() - 1 && self.distances[i] < target {
            i += 1;
        }
        let (d0, d1) = (self.distances[i - 1], self.distances[i]);
        let len = d1 - d0;
        let st = if len > 0.0 { (target - d0) / len } else { 0.0 };
        let (a, b) = (self.points[i - 1], self.points[i]);
        [a[0] * (1.0 - st) + b[0] * st, a[1] * (1.0 - st) + b[1] * st]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_and_merge() {
        let clipped = clip_line(&[vec![[-10.0, 5.0], [10.0, 5.0], [10.0, 20.0]]], 0.0, 0.0, 8.0, 8.0);
        assert_eq!(clipped, vec![vec![[0.0, 5.0], [8.0, 5.0]]]);
        let t = Some("a".to_string());
        let merged = merge_lines(&[(t.clone(), vec![[0.0, 0.0], [1.0, 0.0]]), (t.clone(), vec![[2.0, 0.0], [3.0, 0.0]]), (t, vec![[1.0, 0.0], [2.0, 0.0]])]);
        assert_eq!(merged[0], Some(vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.0], [3.0, 0.0]]));
        assert_eq!((merged[1].as_ref(), merged[2].as_ref()), (None, None));
    }

    #[test]
    fn pole_of_square() {
        let sq = vec![vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]]];
        let p = pole_of_inaccessibility(&sq, 0.1);
        assert!((p[0] - 5.0).abs() < 0.2 && (p[1] - 5.0).abs() < 0.2);
    }
}
