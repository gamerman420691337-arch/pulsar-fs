//! Fast parallelized optical flow, divergence calculation, and radial motion projection in pure Rust.

use rayon::prelude::*;

#[derive(Debug, Clone)]
pub struct FlowField {
    pub width: usize,
    pub height: usize,
    /// Velocity vector components: (u, v) for each pixel in row-major order
    pub u: Vec<f32>,
    pub v: Vec<f32>,
}

impl FlowField {
    pub fn new(width: usize, height: usize) -> Self {
        let size = width * height;
        Self {
            width,
            height,
            u: vec![0.0; size],
            v: vec![0.0; size],
        }
    }

    /// Mean magnitude of the velocity field
    pub fn mean_magnitude(&self) -> f32 {
        let sum: f32 = self
            .u
            .par_iter()
            .zip(self.v.par_iter())
            .map(|(&u, &v)| (u * u + v * v).sqrt())
            .sum();
        sum / (self.u.len() as f32)
    }

    /// Bilinear interpolation sampling of optical flow velocity (u, v) at normalized coordinates [0.0, 1.0]
    pub fn sample_flow_at(&self, norm_x: f32, norm_y: f32) -> (f32, f32) {
        if self.width == 0 || self.height == 0 {
            return (0.0, 0.0);
        }
        let px = (norm_x * (self.width as f32 - 1.0)).clamp(0.0, self.width as f32 - 1.0);
        let py = (norm_y * (self.height as f32 - 1.0)).clamp(0.0, self.height as f32 - 1.0);

        let x0 = px.floor() as usize;
        let y0 = py.floor() as usize;
        let x1 = (x0 + 1).min(self.width - 1);
        let y1 = (y0 + 1).min(self.height - 1);

        let fx = px - x0 as f32;
        let fy = py - y0 as f32;

        let idx00 = y0 * self.width + x0;
        let idx10 = y0 * self.width + x1;
        let idx01 = y1 * self.width + x0;
        let idx11 = y1 * self.width + x1;

        let u0 = self.u[idx00] * (1.0 - fx) + self.u[idx10] * fx;
        let u1 = self.u[idx01] * (1.0 - fx) + self.u[idx11] * fx;
        let sampled_u = u0 * (1.0 - fy) + u1 * fy;

        let v0 = self.v[idx00] * (1.0 - fx) + self.v[idx10] * fx;
        let v1 = self.v[idx01] * (1.0 - fx) + self.v[idx11] * fx;
        let sampled_v = v0 * (1.0 - fy) + v1 * fy;

        (sampled_u, sampled_v)
    }

    /// Compute optical flow vorticity (rotational curl: dv/dx - du/dy) in a circular region around (norm_cx, norm_cy).
    /// Used for T-Code Twist (R2) axial rotation detection.
    pub fn compute_vorticity(&self, norm_cx: f32, norm_cy: f32, norm_radius: f32) -> f32 {
        if self.width < 3 || self.height < 3 {
            return 0.0;
        }

        let cx = (norm_cx * self.width as f32).round() as isize;
        let cy = (norm_cy * self.height as f32).round() as isize;
        let r = ((norm_radius * self.width as f32).round() as isize).max(2);

        let mut sum_curl = 0.0f32;
        let mut count = 0usize;

        let y_min = (cy - r).max(1);
        let y_max = (cy + r).min(self.height as isize - 2);
        let x_min = (cx - r).max(1);
        let x_max = (cx + r).min(self.width as isize - 2);

        for y in y_min..=y_max {
            for x in x_min..=x_max {
                let dx = x - cx;
                let dy = y - cy;
                if dx * dx + dy * dy <= r * r {
                    let idx_xp = (y as usize) * self.width + (x + 1) as usize;
                    let idx_xm = (y as usize) * self.width + (x - 1) as usize;
                    let idx_yp = ((y + 1) as usize) * self.width + (x as usize);
                    let idx_ym = ((y - 1) as usize) * self.width + (x as usize);

                    let dv_dx = (self.v[idx_xp] - self.v[idx_xm]) * 0.5;
                    let du_dy = (self.u[idx_yp] - self.u[idx_ym]) * 0.5;
                    sum_curl += dv_dx - du_dy;
                    count += 1;
                }
            }
        }

        if count > 0 {
            sum_curl / (count as f32)
        } else {
            0.0
        }
    }
}

/// Detect sudden cut via L1 mean pixel difference between frames
pub fn detect_cut_photometric(p0: &[u8], p1: &[u8], threshold: f32) -> bool {
    if p0.len() != p1.len() || p0.is_empty() {
        return true;
    }
    let sum_diff: u64 = p0
        .par_iter()
        .zip(p1.par_iter())
        .map(|(&a, &b)| (a as i32 - b as i32).unsigned_abs() as u64)
        .sum();
    let mean_diff = (sum_diff as f32) / (p0.len() as f32);
    mean_diff > threshold
}

/// High-performance multi-threaded dense optical flow calculation (Lucas-Kanade gradient tensor).
pub fn compute_dense_flow(p0: &[u8], p1: &[u8], width: usize, height: usize) -> FlowField {
    let mut field = FlowField::new(width, height);
    let win_rad = 3isize;
    let lambda = 0.05f32; // Regularization parameter

    // Process rows in parallel via Rayon
    field
        .u
        .par_chunks_mut(width)
        .zip(field.v.par_chunks_mut(width))
        .enumerate()
        .for_each(|(y_idx, (row_u, row_v))| {
            let y = y_idx as isize;
            if y < 1 || y >= (height as isize - 1) {
                return;
            }

            for x in 1..(width as isize - 1) {
                let mut sum_xx = 0.0f32;
                let mut sum_xy = 0.0f32;
                let mut sum_yy = 0.0f32;
                let mut sum_xt = 0.0f32;
                let mut sum_yt = 0.0f32;

                // Accumulate gradients in local window
                for wy in -win_rad..=win_rad {
                    let ny = y + wy;
                    if ny < 1 || ny >= (height as isize - 1) {
                        continue;
                    }
                    for wx in -win_rad..=win_rad {
                        let nx = x + wx;
                        if nx < 1 || nx >= (width as isize - 1) {
                            continue;
                        }

                        let idx = (ny as usize) * width + (nx as usize);
                        let idx_xp = idx + 1;
                        let idx_xm = idx - 1;
                        let idx_yp = idx + width;
                        let idx_ym = idx - width;

                        // Central differences averaged over both frames
                        let ix = ((p0[idx_xp] as f32 - p0[idx_xm] as f32)
                            + (p1[idx_xp] as f32 - p1[idx_xm] as f32))
                            * 0.25;
                        let iy = ((p0[idx_yp] as f32 - p0[idx_ym] as f32)
                            + (p1[idx_yp] as f32 - p1[idx_ym] as f32))
                            * 0.25;
                        let it = p1[idx] as f32 - p0[idx] as f32;

                        sum_xx += ix * ix;
                        sum_xy += ix * iy;
                        sum_yy += iy * iy;
                        sum_xt += ix * it;
                        sum_yt += iy * it;
                    }
                }

                // Solve regularized 2x2 system: [sum_xx+lambda, sum_xy; sum_xy, sum_yy+lambda] [u; v] = -[sum_xt; sum_yt]
                let a = sum_xx + lambda;
                let b = sum_xy;
                let c = sum_yy + lambda;
                let det = a * c - b * b;

                if det.abs() > 1e-6 {
                    let inv_det = 1.0 / det;
                    let u = -(c * sum_xt - b * sum_yt) * inv_det;
                    let v = -(-b * sum_xt + a * sum_yt) * inv_det;

                    // Clamp extreme velocity spikes
                    row_u[x as usize] = u.clamp(-30.0, 30.0);
                    row_v[x as usize] = v.clamp(-30.0, 30.0);
                }
            }
        });

    field
}

/// Computes divergence map div = du/dx + dv/dy and finds maximum divergence center (expansion point)
pub fn find_divergence_center(flow: &FlowField) -> (f32, f32, f32) {
    let w = flow.width;
    let h = flow.height;

    let mut best_div = 0.0f32;
    let mut best_x = (w / 2) as f32;
    let mut best_y = (h / 2) as f32;

    for y in 2..(h - 2) {
        for x in 2..(w - 2) {
            let idx = y * w + x;
            let du_dx = (flow.u[idx + 1] - flow.u[idx - 1]) * 0.5;
            let dv_dy = (flow.v[idx + w] - flow.v[idx - w]) * 0.5;
            let div = du_dx + dv_dy;

            if div.abs() > best_div.abs() {
                best_div = div;
                best_x = x as f32;
                best_y = y as f32;
            }
        }
    }

    (best_x, best_y, best_div)
}

/// Temporal median filtering of center coordinates to suppress sudden noise jumps
pub fn filter_centers_median(centers: &[(f32, f32)], window_radius: usize) -> Vec<(f32, f32)> {
    let n = centers.len();
    let mut filtered = Vec::with_capacity(n);

    let mut xs = Vec::new();
    let mut ys = Vec::new();

    for i in 0..n {
        let start = i.saturating_sub(window_radius);
        let end = (i + window_radius + 1).min(n);

        xs.clear();
        ys.clear();
        for j in start..end {
            xs.push(centers[j].0);
            ys.push(centers[j].1);
        }

        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        ys.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let med_x = xs[xs.len() / 2];
        let med_y = ys[ys.len() / 2];
        filtered.push((med_x, med_y));
    }

    filtered
}

/// Projects flow vectors radially from the motion center.
/// Positive values correspond to outward expansion; negative to inward contraction.
pub fn project_radial_motion(
    flow: &FlowField,
    center: (f32, f32),
    is_cut: bool,
    pov_mode: bool,
    balance_global: bool,
) -> f32 {
    if is_cut {
        return 0.0;
    }

    let w = flow.width as f32;
    let h = flow.height as f32;
    let cx = center.0;
    let cy = center.1;

    let total_pixels = flow.width * flow.height;
    if total_pixels == 0 {
        return 0.0;
    }

    let sum_dot: f32 = (0..flow.height)
        .into_par_iter()
        .map(|y| {
            let mut row_sum = 0.0f32;
            let y_f = y as f32;
            let dy = y_f - cy;

            for x in 0..flow.width {
                let x_f = x as f32;
                let dx = x_f - cx;
                let idx = y * flow.width + x;

                let u = flow.u[idx];
                let v = flow.v[idx];
                let mut dot = u * dx + v * dy;

                if !pov_mode && balance_global {
                    // Attenuate edge pixels to reduce camera panning drift
                    let wx = if x_f > cx {
                        (w - x_f) / w
                    } else {
                        x_f / w
                    };
                    let wy = if y_f > cy {
                        (h - y_f) / h
                    } else {
                        y_f / h
                    };
                    dot *= wx * wy;
                }

                row_sum += dot;
            }
            row_sum
        })
        .sum();

    sum_dot / (total_pixels as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_photometric_cut_detection() {
        let f0 = vec![128u8; 1000];
        let f1 = vec![128u8; 1000];
        assert!(!detect_cut_photometric(&f0, &f1, 30.0));

        let f_black = vec![0u8; 1000];
        assert!(detect_cut_photometric(&f0, &f_black, 30.0));
    }

    #[test]
    fn test_dense_flow_motion_direction() {
        let w = 64;
        let h = 64;
        let mut p0 = vec![0u8; w * h];
        let mut p1 = vec![0u8; w * h];

        // Shift block rightward
        for y in 20..40 {
            for x in 20..40 {
                p0[y * w + x] = 255;
            }
            for x in 23..43 {
                p1[y * w + x] = 255;
            }
        }

        let flow = compute_dense_flow(&p0, &p1, w, h);
        // Measure horizontal flow at the leading edge (x: 38..42, y: 25..35)
        let mut edge_u_sum = 0.0f32;
        let mut count = 0;
        for y in 25..35 {
            for x in 38..42 {
                edge_u_sum += flow.u[y * w + x];
                count += 1;
            }
        }
        let avg_edge_u = edge_u_sum / (count as f32);
        assert!(avg_edge_u > 0.0, "Expected positive horizontal flow at leading edge, got {}", avg_edge_u);
    }

    #[test]
    fn test_filter_centers_suppresses_spikes() {
        let mut raw = vec![(50.0, 50.0); 10];
        raw[5] = (500.0, 500.0); // Outlier spike
        let filtered = filter_centers_median(&raw, 2);
        assert!((filtered[5].0 - 50.0).abs() < 1.0);
        assert!((filtered[5].1 - 50.0).abs() < 1.0);
    }

    #[test]
    fn test_flow_sample_and_vorticity() {
        let mut flow = FlowField::new(10, 10);
        for y in 0..10 {
            for x in 0..10 {
                let idx = y * 10 + x;
                flow.v[idx] = (x as f32 - 4.5) * 2.0;
                flow.u[idx] = -(y as f32 - 4.5) * 2.0;
            }
        }

        let (u_center, v_center) = flow.sample_flow_at(0.5, 0.5);
        assert!(u_center.abs() < 1.0);
        assert!(v_center.abs() < 1.0);

        let vorticity = flow.compute_vorticity(0.5, 0.5, 0.3);
        assert!(vorticity > 0.0, "Expected positive rotational vorticity, got {}", vorticity);
    }
}
