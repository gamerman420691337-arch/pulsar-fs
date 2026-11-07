//! The Handy REST v3 client for HSP (Handy Streaming Protocol) playback.
//!
//! The device plays timed points from its own buffer, so playback does not depend on
//! cloud round-trip time (~0.4-1 s per call). Authentication: the application key
//! (`HANDY_API_KEY`) is used only to issue a client token; device endpoints take
//! `Authorization: Bearer <token>` plus `X-Connection-Key`.
//!
//! Safety: the device keeps playing whatever is buffered after a disconnect or crash,
//! so callers must only preload a short window ahead of the play position.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const BASE: &str = "https://www.handyfeeling.com/api/handy-rest/v3";
/// Re-issue the cached token when less than this remains.
const TOKEN_MARGIN: Duration = Duration::from_secs(120);
const TOKEN_TTL_SECS: u64 = 3600;
/// Maximum points per `/hsp/add` request.
pub const MAX_ADD_POINTS: usize = 100;

/// Estimate of the Handy server clock, anchored to a host instant.
#[derive(Debug, Clone, Copy)]
pub struct ServerClock {
    server_ms: u64,
    anchor: Instant,
    pub rtt_ms: u64,
}

impl ServerClock {
    pub fn advanced(&self, millis: u64) -> Self {
        Self {
            server_ms: self.server_ms.saturating_add(millis),
            anchor: self.anchor,
            rtt_ms: self.rtt_ms,
        }
    }
    pub fn now_ms(&self) -> u64 {
        self.server_ms + self.anchor.elapsed().as_millis() as u64
    }
}

pub struct HspClient {
    agent: ureq::Agent,
    api_key: String,
    conn_key: String,
    token: Option<(String, Instant)>,
    base: String,
}

impl HspClient {
    pub fn new(api_key: &str, conn_key: &str) -> Self {
        let agent: ureq::Agent = ureq::config::Config::builder()
            .timeout_global(Some(Duration::from_secs(3)))
            .max_redirects(0)
            .build()
            .into();
        Self {
            agent,
            api_key: api_key.trim().to_string(),
            conn_key: conn_key.trim().to_string(),
            token: None,
            base: BASE.into(),
        }
    }

    #[cfg(test)]
    pub(crate) fn test_client(base: &str) -> Self {
        assert!(base.starts_with("http://127.0.0.1:"));
        let mut client = Self::new("test-app", "test-connection");
        client.base = base.into();
        client
    }

    fn token(&mut self) -> Result<String> {
        if let Some((tok, expires)) = &self.token {
            if Instant::now() + TOKEN_MARGIN < *expires {
                return Ok(tok.clone());
            }
        }
        let url = format!(
            "{}/auth/token/issue?to={}&ttl={TOKEN_TTL_SECS}",
            self.base, self.conn_key
        );
        let body = self
            .agent
            .get(&url)
            .header("X-Api-Key", &self.api_key)
            .header("Accept", "application/json")
            .call()
            .map_err(|_| {
                anyhow!("Handy token issue failed (check HANDY_API_KEY and connection key)")
            })?
            .into_body()
            .read_to_string()?;
        let val: Value = serde_json::from_str(&body)?;
        let tok = val["result"]["token"]
            .as_str()
            .filter(|t| !t.is_empty())
            .ok_or_else(|| anyhow!("Handy token response had no token"))?
            .to_string();
        self.token = Some((
            tok.clone(),
            Instant::now() + Duration::from_secs(TOKEN_TTL_SECS),
        ));
        Ok(tok)
    }

    fn call(&mut self, method: &str, path: &str, body: Option<Value>) -> Result<Value> {
        for attempt in 0..2 {
            let token = self.token()?;
            let url = format!("{}{path}", self.base);
            let result = match (method, &body) {
                ("GET", _) => self
                    .agent
                    .get(&url)
                    .header("Authorization", &format!("Bearer {token}"))
                    .header("X-Connection-Key", &self.conn_key)
                    .call(),
                (_, Some(b)) => self
                    .agent
                    .put(&url)
                    .header("Authorization", &format!("Bearer {token}"))
                    .header("X-Connection-Key", &self.conn_key)
                    .header("Content-Type", "application/json")
                    .send(serde_json::to_vec(b)?),
                (_, None) => self
                    .agent
                    .put(&url)
                    .header("Authorization", &format!("Bearer {token}"))
                    .header("X-Connection-Key", &self.conn_key)
                    .send_empty(),
            };
            match result {
                Ok(resp) => {
                    let text = resp.into_body().read_to_string()?;
                    if text.trim().is_empty() {
                        return Ok(Value::Null);
                    }
                    let val: Value = serde_json::from_str(&text)
                        .with_context(|| format!("Handy {path}: invalid JSON response"))?;
                    if let Some(err) = val.get("error").filter(|e| !e.is_null()) {
                        let _ = err;
                        bail!("Handy {path} refused the operation");
                    }
                    return Ok(val);
                }
                Err(ureq::Error::StatusCode(401)) if attempt == 0 => {
                    self.token = None;
                }
                Err(ureq::Error::StatusCode(code)) => {
                    bail!("Handy {method} {path} failed: HTTP {code}")
                }
                Err(e) => {
                    let _ = e;
                    bail!("Handy {method} {path} transport unavailable")
                }
            }
        }
        unreachable!()
    }

    pub fn connected(&mut self) -> Result<bool> {
        let v = self.call("GET", "/connected", None)?;
        Ok(v["result"]["connected"].as_bool().unwrap_or(false))
    }

    pub fn mode(&mut self) -> Result<i64> {
        let v = self.call("GET", "/mode", None)?;
        v["result"]["mode"]
            .as_i64()
            .ok_or_else(|| anyhow!("Handy /mode response had no mode"))
    }

    /// HSP is device mode 4.
    pub fn ensure_hsp_mode(&mut self) -> Result<()> {
        if self.mode()? != 4 {
            self.call("PUT", "/mode", Some(json!({ "mode": 4 })))?;
            if self.mode()? != 4 {
                bail!("Handy did not enter HSP mode (mode 4)");
            }
        }
        Ok(())
    }

    pub fn state(&mut self) -> Result<Value> {
        Ok(self.call("GET", "/hsp/state", None)?["result"].clone())
    }

    /// Best-of-N server clock estimate (lowest round trip wins).
    pub fn server_clock(&mut self, samples: usize) -> Result<ServerClock> {
        let mut best: Option<ServerClock> = None;
        for _ in 0..samples.max(1) {
            let t0 = Instant::now();
            let v = self.call("GET", "/servertime", None)?;
            let rtt = t0.elapsed();
            let server_ms = v["server_time"]
                .as_u64()
                .ok_or_else(|| anyhow!("Handy /servertime had no server_time"))?;
            let half = rtt / 2;
            let clock = ServerClock {
                server_ms: server_ms + half.as_millis() as u64,
                anchor: Instant::now(),
                rtt_ms: rtt.as_millis() as u64,
            };
            if best.map_or(true, |b| clock.rtt_ms < b.rtt_ms) {
                best = Some(clock);
            }
        }
        best.ok_or_else(|| anyhow!("no server clock samples"))
    }

    /// The user sync offset stored on the device (set from the Handyverse site), in ms.
    pub fn user_offset_ms(&mut self) -> Result<i64> {
        let v = self.call("GET", "/hstp/offset", None)?;
        Ok(v["result"]["offset"]
            .as_i64()
            .or_else(|| v["result"].as_i64())
            .unwrap_or(0))
    }

    pub fn setup(&mut self, stream_id: u32) -> Result<()> {
        if stream_id == 0 {
            bail!("stream id must be nonzero");
        }
        self.call("PUT", "/hsp/setup", Some(json!({ "stream_id": stream_id })))?;
        Ok(())
    }

    /// Append a chunk. `tail` is the absolute zero-based stream index of the chunk's last point.
    pub fn add(&mut self, points: &[(u32, u8)], tail: u32) -> Result<()> {
        if points.is_empty() || points.len() > MAX_ADD_POINTS {
            bail!("HSP add needs 1..={MAX_ADD_POINTS} points");
        }
        let pts: Vec<Value> = points
            .iter()
            .map(|&(t, x)| json!({ "t": t, "x": x }))
            .collect();
        self.call(
            "PUT",
            "/hsp/add",
            Some(json!({ "points": pts, "tail_point_stream_index": tail, "flush": false })),
        )?;
        Ok(())
    }

    /// Play from `start_ms` of script time, aligned to the given server-clock estimate.
    pub fn play(&mut self, start_ms: i32, rate: f64, clock: &ServerClock) -> Result<()> {
        self.call(
            "PUT",
            "/hsp/play",
            Some(json!({
                "start_time": start_ms,
                "server_time": clock.now_ms(),
                "playback_rate": rate,
                "pause_on_starving": false,
                "loop": false
            })),
        )?;
        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        self.call("PUT", "/hsp/stop", None)?;
        Ok(())
    }
}

/// A windowed upload of one script to the device buffer. Only points up to a limit
/// are ever sent, so a lost client leaves the device with a short bounded tail.
pub struct HspStream {
    points: std::sync::Arc<Vec<(u32, u8)>>,
    next: usize,
    count: u32,
}

impl HspStream {
    /// Start a fresh device stream whose first point is the last one at or before `from_ms`.
    pub fn begin(
        client: &mut HspClient,
        points: std::sync::Arc<Vec<(u32, u8)>>,
        from_ms: i64,
    ) -> Result<Self> {
        let first = points
            .partition_point(|p| (p.0 as i64) <= from_ms)
            .saturating_sub(1);
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as u32)
            .unwrap_or(1);
        let id = secs
            .wrapping_add(COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
            .max(1);
        client.setup(id)?;
        Ok(Self {
            points,
            next: first,
            count: 0,
        })
    }

    /// Send every point with time <= `until_ms` (the first upload always has at least two).
    pub fn load_until(&mut self, client: &mut HspClient, until_ms: u64) -> Result<()> {
        loop {
            let remaining = self.points.len() - self.next;
            if remaining == 0 {
                return Ok(());
            }
            let min_take = if self.count == 0 { 2.min(remaining) } else { 0 };
            let mut take = self.points[self.next..]
                .iter()
                .take(MAX_ADD_POINTS)
                .take_while(|p| p.0 as u64 <= until_ms)
                .count();
            take = take.max(min_take).min(MAX_ADD_POINTS);
            if take == 0 {
                return Ok(());
            }
            let chunk = &self.points[self.next..self.next + take];
            client.add(chunk, self.count + take as u32 - 1)?;
            self.next += take;
            self.count += take as u32;
            return Ok(());
        }
    }

    pub fn loaded_until(&self, until_ms: u64) -> bool {
        self.next >= self.points.len()
            || (self.count >= 2 && self.points[self.next].0 as u64 > until_ms)
    }

    pub fn sent(&self) -> u32 {
        self.count
    }
}

/// Convert script actions to device points: sorted, strictly increasing in time, limited to
/// `max_speed` (units/s, 100 units = full stroke) and mapped into `[min_pct, max_pct]`.
/// Speed limiting moves toward each target as far as the time gap allows, so the device
/// never receives a stroke faster than the limit.
pub fn build_points(
    actions: &[(i64, i32)],
    max_speed: f32,
    min_pct: f32,
    max_pct: f32,
) -> Vec<(u32, u8)> {
    let mut sorted: Vec<(i64, i32)> = actions.iter().copied().filter(|&(t, _)| t >= 0).collect();
    sorted.sort_by_key(|&(t, _)| t);
    sorted.dedup_by(|b, a| {
        if a.0 == b.0 {
            a.1 = b.1; // keep the later value at a repeated time
            true
        } else {
            false
        }
    });

    let span = (max_pct - min_pct).max(0.0);
    let mut out = Vec::with_capacity(sorted.len());
    let mut prev: Option<(i64, f32)> = None;
    for (t, pos) in sorted {
        let mut p = (pos.clamp(0, 100)) as f32;
        if let Some((pt, pp)) = prev {
            let max_delta = max_speed * (t - pt) as f32 / 1000.0;
            p = p.clamp(pp - max_delta, pp + max_delta);
        }
        prev = Some((t, p));
        let x = (min_pct + p / 100.0 * span).round().clamp(0.0, 100.0) as u8;
        out.push((t as u32, x));
    }
    // HSP requires two initial points. Split long linear segments so that this
    // minimum cannot preload minutes of future movement beyond the 5s window.
    if out.first().is_some_and(|p| p.0 > 0) {
        out.insert(0, (0, out[0].1));
    }
    let mut bounded = Vec::with_capacity(out.len());
    for pair in out.windows(2) {
        let (start, x) = pair[0];
        let (end, y) = pair[1];
        bounded.push((start, x));
        let mut t = u64::from(start) + 5000;
        while t < u64::from(end) {
            let fraction = (t - u64::from(start)) as f64 / f64::from(end - start);
            let value = (f64::from(x) + (f64::from(y) - f64::from(x)) * fraction).round() as u8;
            bounded.push((t as u32, value));
            t += 5000;
        }
    }
    if let Some(point) = out.last() {
        bounded.push(*point);
    }
    bounded
}

/// Express the normalized points from `build_points` in video time. A delay holds
/// the initial position; an advance starts at the interpolated skipped position.
pub fn offset_points(points: &[(u32, u8)], offset_ms: i16) -> Vec<(u32, u8)> {
    let Some(&(end, final_position)) = points.last() else {
        return Vec::new();
    };
    if offset_ms >= 0 {
        let delay = offset_ms as u32;
        let mut shifted = Vec::with_capacity(points.len() + 1);
        if delay > 0 {
            shifted.push((0, points[0].1));
        }
        shifted.extend(points.iter().map(|&(t, x)| (t + delay, x)));
        return shifted;
    }

    let advance = u32::from(offset_ms.unsigned_abs());
    let first_future = points.partition_point(|&(t, _)| t <= advance);
    let initial = if let Some(&(next_t, next_x)) = points.get(first_future) {
        let (prev_t, prev_x) = points[first_future - 1];
        let fraction = f64::from(advance - prev_t) / f64::from(next_t - prev_t);
        (f64::from(prev_x) + fraction * (f64::from(next_x) - f64::from(prev_x))).round() as u8
    } else {
        final_position
    };
    let mut shifted = Vec::with_capacity(points.len() - first_future + 2);
    shifted.push((0, initial));
    shifted.extend(
        points[first_future..]
            .iter()
            .map(|&(t, x)| (t - advance, x)),
    );
    // Preserve the video endpoint when the last motion point moves earlier.
    if end > 0 {
        shifted.push((end, final_position));
    }
    shifted
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offset_holds_or_interpolates_at_video_boundaries() {
        let points = [(0, 20), (1_000, 80), (2_000, 80)];
        assert_eq!(offset_points(&points, 0), points);
        assert_eq!(
            offset_points(&points, 250),
            [(0, 20), (250, 20), (1_250, 80), (2_250, 80)]
        );
        assert_eq!(
            offset_points(&points, -250),
            [(0, 35), (750, 80), (1_750, 80), (2_000, 80)]
        );
        assert_eq!(offset_points(&points, -2_000), [(0, 80), (2_000, 80)]);
    }

    #[test]
    fn long_segments_keep_a_five_second_preload_bound() {
        let points = build_points(&[(0, 40), (60_000, 60)], 150.0, 0.0, 100.0);
        assert_eq!(points.first(), Some(&(0, 40)));
        assert_eq!(points.last(), Some(&(60_000, 60)));
        assert!(points.windows(2).all(|p| p[1].0 - p[0].0 <= 5000));
        let delayed = build_points(&[(30_000, 40), (60_000, 40)], 150.0, 0.0, 100.0);
        assert_eq!(delayed.first(), Some(&(0, 40)));
        assert_eq!(delayed[1], (5000, 40));
    }

    #[test]
    fn speed_limit_caps_stroke_rate() {
        // 100 units in 100 ms is 1000 u/s; limit 300 u/s allows 30 units.
        let pts = build_points(&[(0, 0), (100, 100)], 300.0, 0.0, 100.0);
        assert_eq!(pts, vec![(0, 0), (100, 30)]);
    }

    #[test]
    fn slow_strokes_pass_unchanged() {
        let pts = build_points(&[(0, 0), (1000, 100), (2000, 0)], 300.0, 0.0, 100.0);
        assert_eq!(pts, vec![(0, 0), (1000, 100), (2000, 0)]);
        let ok = build_points(&[(0, 0), (500, 100)], 300.0, 0.0, 100.0);
        assert_eq!(ok, vec![(0, 0), (500, 100)]);
    }

    #[test]
    fn range_mapping_and_dedup() {
        let pts = build_points(&[(0, 0), (500, 50), (500, 100)], 1000.0, 10.0, 90.0);
        assert_eq!(pts, vec![(0, 10), (500, 90)]);
    }
}
