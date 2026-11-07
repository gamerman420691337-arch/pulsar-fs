//! Bounded coarse proposals with full-resolution checked refinement. Accepted
//! single-level flows are preserved; coarse-only matches are never published.
use super::*;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PyramidFlowSummary {
    pub maximum_level: usize,
    pub recovered_points: usize,
    pub component_displacement_bound: f64,
}

pub fn track_points_fb_multiscale(
    previous: &[u8],
    current: &[u8],
    width: u32,
    height: u32,
    points: &[[f64; 2]],
    config: FlowConfig,
) -> Result<(Vec<PointFlow>, PyramidFlowSummary), CoreError> {
    // Existing admission validates shape, pixels, points and work configuration
    // before any pyramid allocation or sampling.
    let mut flows = track_points_fb(previous, current, width, height, points, config)?;
    let levels = if width.min(height) >= 256 { 2 } else { 0 };
    let bound = if levels == 0 {
        8.5
    } else {
        (f64::from(config.search_radius) + 0.5) * 4.0
    };
    let mut summary = PyramidFlowSummary {
        maximum_level: levels,
        recovered_points: 0,
        component_displacement_bound: bound,
    };
    if levels == 0 || flows.iter().all(|flow| flow.displacement.is_some()) {
        return Ok((flows, summary));
    }
    let mut previous_levels: Vec<Vec<u8>> = Vec::new();
    let mut current_levels: Vec<Vec<u8>> = Vec::new();
    let (mut w, mut h) = (width as usize, height as usize);
    for level in 0..levels {
        let p = if level == 0 {
            previous
        } else {
            previous_levels[level - 1].as_slice()
        };
        let c = if level == 0 {
            current
        } else {
            current_levels[level - 1].as_slice()
        };
        previous_levels.push(half(p, w, h));
        current_levels.push(half(c, w, h));
        w /= 2;
        h /= 2;
    }
    for (index, flow) in flows.iter_mut().enumerate() {
        if flow.displacement.is_some() {
            continue;
        }
        for level in 1..=levels {
            let scale = (1usize << level) as f64;
            // Box-average sample center: original center = scale*(i+.5)-.5.
            let p = points[index].map(|v| (v + 0.5) / scale - 0.5);
            let coarse = track_points_fb(
                &previous_levels[level - 1],
                &current_levels[level - 1],
                width >> level,
                height >> level,
                &[p],
                config,
            )?;
            let Some(shift) = coarse[0].displacement.map(|d| d.map(|v| v * scale)) else {
                continue;
            };
            let Some(mut refined) = refine(
                previous,
                current,
                width,
                height,
                points[index],
                shift,
                config,
            )?
            else {
                continue;
            };
            let Some(delta) = refined.displacement else {
                continue;
            };
            if delta.iter().any(|v| !v.is_finite() || v.abs() > bound) {
                continue;
            }
            refined.source = points[index];
            *flow = refined;
            summary.recovered_points += 1;
            break;
        }
    }
    Ok((flows, summary))
}

fn half(image: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut result = Vec::with_capacity((width / 2) * (height / 2));
    for y in 0..height / 2 {
        for x in 0..width / 2 {
            let i = 2 * y * width + 2 * x;
            let sum = u32::from(image[i])
                + u32::from(image[i + 1])
                + u32::from(image[i + width])
                + u32::from(image[i + width + 1]);
            result.push(((sum + 2) / 4) as u8);
        }
    }
    result
}

fn tile(image: &[u8], width: u32, height: u32, center: [f64; 2]) -> Option<Vec<u8>> {
    // A fixed 32x32 tile covers the maximum admitted radius/search margin (15)
    // and retains the full-resolution forward destination for the reverse check.
    if center.iter().any(|v| !v.is_finite())
        || center[0] < 16.0
        || center[1] < 16.0
        || center[0] + 16.0 >= f64::from(width)
        || center[1] + 16.0 >= f64::from(height)
    {
        return None;
    }
    let mut result = Vec::with_capacity(1024);
    for y in 0..32 {
        for x in 0..32 {
            result.push(
                sample(
                    image,
                    width as usize,
                    center[0] + f64::from(x) - 16.0,
                    center[1] + f64::from(y) - 16.0,
                )
                .round() as u8,
            );
        }
    }
    Some(result)
}

fn refine(
    previous: &[u8],
    current: &[u8],
    width: u32,
    height: u32,
    point: [f64; 2],
    proposal: [f64; 2],
    config: FlowConfig,
) -> Result<Option<PointFlow>, CoreError> {
    let Some(p) = tile(previous, width, height, point) else {
        return Ok(None);
    };
    let Some(c) = tile(
        current,
        width,
        height,
        [point[0] + proposal[0], point[1] + proposal[1]],
    ) else {
        return Ok(None);
    };
    let mut flow = track_points_fb(&p, &c, 32, 32, &[[16.0, 16.0]], config)?.remove(0);
    if let Some(delta) = flow.displacement {
        // Tile origins differ by proposal. Reverse matching is at the actual
        // forward destination in c; origin offsets cancel in its cycle error.
        flow.displacement = Some([proposal[0] + delta[0], proposal[1] + delta[1]]);
        Ok(Some(flow))
    } else {
        Ok(None)
    }
}
