//! Offline codecs for the explicit Handy REST v3 HSP streaming subset.
//!
//! See `research/phase-b/handy-hsp-stream-contract-2026-10-23.md` for the
//! manufacturer sources and their unresolved contradictions. Encoding a request
//! supplies neither credentials nor permission to dispatch it. These codecs do
//! not select a mode, retry an operation, or renew a clock/physical qualification.
//!
//! Positions are canonical integer percent, 0..=100, without scaling or clamping.
//! REST's PointPosition prose/example and manufacturer SDK/RPC use this range,
//! but REST's schema declares maximum 50. Live acceptance above 50 remains an
//! explicit compatibility question; these bytes do not establish that acceptance.
use serde::{Deserialize, Serialize};

pub const HSP_RESPONSE_LIMIT: usize = 64 * 1024;
pub const HSP_MAX_ADD_POINTS: usize = 100;
/// Largest integer represented exactly by the JSON/JavaScript number contract.
pub const HSP_MAX_SERVER_TIME_MS: u64 = (1_u64 << 53) - 1;
/// The portable subset must fit both REST requests and signed RPC state readback.
pub const HSP_MAX_TAIL_INDEX: u32 = i32::MAX as u32;

/// Errors carry no response text, URLs, credentials, or arbitrary peer fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HspWireError {
    InvalidStreamId,
    InvalidPoint,
    InvalidPointCount,
    NonIncreasingPointTime,
    InvalidTailIndex,
    InvalidClock,
    EncodingFailure,
    OversizedResponse,
    HttpFailure,
    InvalidResponse,
}

/// A timed, canonical percentage position. Private fields prevent bypassing the
/// range check; construction never rounds timestamps or rescales coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct HspWirePoint {
    t: u32,
    x: u8,
}

impl HspWirePoint {
    pub fn new(t: u32, x: u8) -> Result<Self, HspWireError> {
        if x > 100 {
            return Err(HspWireError::InvalidPoint);
        }
        Ok(Self { t, x })
    }

    pub fn t(&self) -> u32 {
        self.t
    }

    pub fn x(&self) -> u8 {
        self.x
    }
}

/// Fixed relative endpoint, method, and exact bytes. Contains no configurable
/// origin or dispatch authority. The caller owns admission and operation identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HspEncodedRequest {
    method: &'static str,
    path: &'static str,
    body: Option<Vec<u8>>,
}

impl HspEncodedRequest {
    pub fn method(&self) -> &'static str {
        self.method
    }

    pub fn path(&self) -> &'static str {
        self.path
    }

    pub fn body(&self) -> Option<&[u8]> {
        self.body.as_deref()
    }

    pub fn setup(stream_id: u32) -> Result<Self, HspWireError> {
        if stream_id == 0 {
            return Err(HspWireError::InvalidStreamId);
        }
        #[derive(Serialize)]
        struct Setup {
            stream_id: u32,
        }
        Self::put("/hsp/setup", &Setup { stream_id })
    }

    /// Append one nonempty chunk with an explicit absolute, zero-based tail.
    /// The session owner must check cumulative index arithmetic and timestamp
    /// ordering across chunks. The tail is not a deduplication/content identity.
    /// A first chunk of one point has tail zero and is outside this REST subset.
    pub fn add(
        points: &[HspWirePoint],
        tail_point_stream_index: u32,
    ) -> Result<Self, HspWireError> {
        if points.is_empty() || points.len() > HSP_MAX_ADD_POINTS {
            return Err(HspWireError::InvalidPointCount);
        }
        if tail_point_stream_index == 0 || tail_point_stream_index > HSP_MAX_TAIL_INDEX {
            return Err(HspWireError::InvalidTailIndex);
        }
        if points.windows(2).any(|pair| pair[0].t >= pair[1].t) {
            return Err(HspWireError::NonIncreasingPointTime);
        }
        // The last absolute index cannot precede the chunk's own first index.
        if u64::from(tail_point_stream_index) + 1 < points.len() as u64 {
            return Err(HspWireError::InvalidTailIndex);
        }
        #[derive(Serialize)]
        struct Add<'a> {
            points: &'a [HspWirePoint],
            tail_point_stream_index: u32,
            flush: bool,
        }
        Self::put(
            "/hsp/add",
            &Add {
                points,
                tail_point_stream_index,
                flush: false,
            },
        )
    }

    /// Explicit, rate-one playback without looping or pausing on starvation.
    /// RPC defines signed int32 script time. The explicit server estimate is
    /// bounded to exact JSON integers; zero's SDK/default semantics are excluded.
    /// It is the estimate at sending, not a future execution deadline.
    pub fn play(start_time: i32, server_time: u64) -> Result<Self, HspWireError> {
        if server_time == 0 || server_time > HSP_MAX_SERVER_TIME_MS {
            return Err(HspWireError::InvalidClock);
        }
        #[derive(Serialize)]
        struct Play {
            start_time: i32,
            server_time: u64,
            playback_rate: f64,
            pause_on_starving: bool,
            r#loop: bool,
        }
        Self::put(
            "/hsp/play",
            &Play {
                start_time,
                server_time,
                playback_rate: 1.0,
                pause_on_starving: false,
                r#loop: false,
            },
        )
    }

    pub fn stop() -> Self {
        Self {
            method: "PUT",
            path: "/hsp/stop",
            body: None,
        }
    }

    pub fn state() -> Self {
        Self {
            method: "GET",
            path: "/hsp/state",
            body: None,
        }
    }

    fn put<T: Serialize>(path: &'static str, value: &T) -> Result<Self, HspWireError> {
        let body = serde_json::to_vec(value).map_err(|_| HspWireError::EncodingFailure)?;
        Ok(Self {
            method: "PUT",
            path,
            body: Some(body),
        })
    }
}

/// Documented numeric states. Undocumented string aliases are not coerced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HspPlayState {
    NotInitialized,
    Playing,
    Stopped,
    Paused,
    Starving,
}

/// Approximate status assembled by the device across different instants.
/// `current_point` may move backward and is not physical position; paused time
/// may drift. Neither these fields nor a matching tail prove point content,
/// physical execution, an atomic snapshot, or a renewed effective clock.
///
/// Operation-specific stream, tail, rate, and stopped-state matching belongs to
/// the session owner. A valid observation can be playing, looping, or rate != 1.
/// The REST state rate is an unconstrained number: finite zero/negative values
/// are retained as metadata, including before Play, without admitting playback.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HspState {
    pub play_state: HspPlayState,
    pub points: u32,
    pub max_points: u32,
    pub current_point: i32,
    pub current_time: i32,
    pub looping: bool,
    pub playback_rate: f64,
    pub first_point_time: u32,
    pub last_point_time: u32,
    pub stream_id: u32,
    pub tail_point_stream_index: i32,
    pub tail_point_stream_index_threshold: u32,
    pub pause_on_starving: bool,
}

/// Decode the complete documented envelope for Setup/Add/Play/Stop/State.
///
/// This strict initial profile rejects extra fields as well as duplicates and
/// missing fields, including every `error` member even when its value is null.
/// RPC integer widths bound the portable subset; RPC remains draft evidence,
/// not a claim that this REST deployment has been physically qualified.
pub fn decode_state(status: u16, bytes: &[u8]) -> Result<HspState, HspWireError> {
    if status != 200 {
        return Err(HspWireError::HttpFailure);
    }
    if bytes.len() > HSP_RESPONSE_LIMIT {
        return Err(HspWireError::OversizedResponse);
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Envelope {
        result: State,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct State {
        play_state: u8,
        points: u32,
        max_points: f64,
        current_point: i32,
        current_time: i32,
        r#loop: bool,
        playback_rate: f64,
        first_point_time: u32,
        last_point_time: u32,
        stream_id: u32,
        tail_point_stream_index: i32,
        tail_point_stream_index_threshold: u32,
        pause_on_starving: bool,
    }
    let Envelope { result: state } =
        serde_json::from_slice(bytes).map_err(|_| HspWireError::InvalidResponse)?;
    let play_state = match state.play_state {
        0 => HspPlayState::NotInitialized,
        1 => HspPlayState::Playing,
        2 => HspPlayState::Stopped,
        3 => HspPlayState::Paused,
        4 => HspPlayState::Starving,
        _ => return Err(HspWireError::InvalidResponse),
    };
    // Capacity is a REST number, unlike the other integer fields. Accept e.g.
    // 100.0, but never round fractional capacities or saturate out-of-range ones.
    if !state.max_points.is_finite()
        || state.max_points < 0.0
        || state.max_points > f64::from(u32::MAX)
        || state.max_points.fract() != 0.0
        || state.current_point < -1
        || state.tail_point_stream_index < -1
        || !state.playback_rate.is_finite()
    {
        return Err(HspWireError::InvalidResponse);
    }
    Ok(HspState {
        play_state,
        points: state.points,
        max_points: state.max_points as u32,
        current_point: state.current_point,
        current_time: state.current_time,
        looping: state.r#loop,
        playback_rate: state.playback_rate,
        first_point_time: state.first_point_time,
        last_point_time: state.last_point_time,
        stream_id: state.stream_id,
        tail_point_stream_index: state.tail_point_stream_index,
        tail_point_stream_index_threshold: state.tail_point_stream_index_threshold,
        pause_on_starving: state.pause_on_starving,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Hand-authored bytes, independent of the encoder and response struct.
    const STATE: &[u8] = br#"{"result":{"play_state":1,"points":3,"max_points":100.0,"current_point":2,"current_time":125,"loop":false,"playback_rate":1.0,"first_point_time":0,"last_point_time":500,"stream_id":4294967295,"tail_point_stream_index":7,"tail_point_stream_index_threshold":1,"pause_on_starving":false}}"#;

    fn point(t: u32, x: u8) -> HspWirePoint {
        HspWirePoint::new(t, x).unwrap()
    }

    fn changed(field: &str, value: serde_json::Value) -> Vec<u8> {
        let mut state: serde_json::Value = serde_json::from_slice(STATE).unwrap();
        state["result"][field] = value;
        serde_json::to_vec(&state).unwrap()
    }

    #[test]
    fn request_golden_bytes_preserve_percent_and_explicit_fixed_options() {
        let setup = HspEncodedRequest::setup(u32::MAX).unwrap();
        assert_eq!(setup.method(), "PUT");
        assert_eq!(setup.path(), "/hsp/setup");
        assert_eq!(
            setup.body(),
            Some(br#"{"stream_id":4294967295}"#.as_slice())
        );
        let add =
            HspEncodedRequest::add(&[point(0, 0), point(250, 100), point(500, 51)], 2).unwrap();
        assert_eq!(add.method(), "PUT");
        assert_eq!(add.path(), "/hsp/add");
        assert_eq!(add.body(), Some(br#"{"points":[{"t":0,"x":0},{"t":250,"x":100},{"t":500,"x":51}],"tail_point_stream_index":2,"flush":false}"#.as_slice()));
        let play = HspEncodedRequest::play(-125, 1_729_000_000_123).unwrap();
        assert_eq!(play.method(), "PUT");
        assert_eq!(play.path(), "/hsp/play");
        assert_eq!(play.body(), Some(br#"{"start_time":-125,"server_time":1729000000123,"playback_rate":1.0,"pause_on_starving":false,"loop":false}"#.as_slice()));
        let stop = HspEncodedRequest::stop();
        assert_eq!(
            (stop.method(), stop.path(), stop.body()),
            ("PUT", "/hsp/stop", None)
        );
        let state = HspEncodedRequest::state();
        assert_eq!(
            (state.method(), state.path(), state.body()),
            ("GET", "/hsp/state", None)
        );
    }

    #[test]
    fn point_encoding_and_request_numeric_bounds_are_exact() {
        let p = point(u32::MAX, 100);
        assert_eq!((p.t(), p.x()), (u32::MAX, 100));
        assert_eq!(
            serde_json::to_vec(&p).unwrap(),
            br#"{"t":4294967295,"x":100}"#
        );
        for x in [101, u8::MAX] {
            assert_eq!(HspWirePoint::new(0, x), Err(HspWireError::InvalidPoint));
        }
        assert_eq!(
            HspEncodedRequest::setup(0),
            Err(HspWireError::InvalidStreamId)
        );
        assert!(HspEncodedRequest::setup(1).is_ok());
        for start in [i32::MIN, -1, 0, i32::MAX] {
            let request = HspEncodedRequest::play(start, HSP_MAX_SERVER_TIME_MS).unwrap();
            let body: serde_json::Value = serde_json::from_slice(request.body().unwrap()).unwrap();
            assert_eq!(body["start_time"].as_i64(), Some(i64::from(start)));
            assert_eq!(body["server_time"].as_u64(), Some(HSP_MAX_SERVER_TIME_MS));
        }
        for server_time in [0, HSP_MAX_SERVER_TIME_MS + 1, u64::MAX] {
            assert_eq!(
                HspEncodedRequest::play(0, server_time),
                Err(HspWireError::InvalidClock)
            );
        }
    }

    #[test]
    fn append_rejects_empty_oversized_unordered_and_unrepresentable_chunks() {
        assert_eq!(
            HspEncodedRequest::add(&[], 1),
            Err(HspWireError::InvalidPointCount)
        );
        let points: Vec<_> = (0..101).map(|t| point(t, 50)).collect();
        assert_eq!(
            HspEncodedRequest::add(&points, 100),
            Err(HspWireError::InvalidPointCount)
        );
        assert!(HspEncodedRequest::add(&points[..100], 99).is_ok());
        for times in [[1, 1], [2, 1], [u32::MAX, 0]] {
            assert_eq!(
                HspEncodedRequest::add(&[point(times[0], 0), point(times[1], 100)], 1),
                Err(HspWireError::NonIncreasingPointTime)
            );
        }
        for tail in [0, HSP_MAX_TAIL_INDEX + 1, u32::MAX] {
            assert_eq!(
                HspEncodedRequest::add(&[point(0, 0), point(1, 100)], tail),
                Err(HspWireError::InvalidTailIndex)
            );
        }
        assert_eq!(
            HspEncodedRequest::add(&points[..3], 1),
            Err(HspWireError::InvalidTailIndex)
        );
        assert!(HspEncodedRequest::add(&[point(u32::MAX, 100)], HSP_MAX_TAIL_INDEX).is_ok());
    }

    #[test]
    fn response_golden_preserves_all_thirteen_fields() {
        assert_eq!(
            decode_state(200, STATE).unwrap(),
            HspState {
                play_state: HspPlayState::Playing,
                points: 3,
                max_points: 100,
                current_point: 2,
                current_time: 125,
                looping: false,
                playback_rate: 1.0,
                first_point_time: 0,
                last_point_time: 500,
                stream_id: u32::MAX,
                tail_point_stream_index: 7,
                tail_point_stream_index_threshold: 1,
                pause_on_starving: false,
            }
        );
    }

    #[test]
    fn state_is_approximate_signed_status_not_operation_acceptance() {
        let sentinels = br#"{"result":{"play_state":4,"points":0,"max_points":0,"current_point":-1,"current_time":-2147483648,"loop":true,"playback_rate":0.5,"first_point_time":0,"last_point_time":0,"stream_id":0,"tail_point_stream_index":-1,"tail_point_stream_index_threshold":0,"pause_on_starving":true}}"#;
        let state = decode_state(200, sentinels).unwrap();
        assert_eq!(state.play_state, HspPlayState::Starving);
        assert_eq!(
            (
                state.current_point,
                state.current_time,
                state.tail_point_stream_index
            ),
            (-1, i32::MIN, -1)
        );
        assert!(state.looping);
        assert!(state.pause_on_starving);
        assert_eq!(state.playback_rate, 0.5);
        let earlier = decode_state(200, STATE).unwrap();
        let later = decode_state(200, &changed("current_point", 1.into())).unwrap();
        assert!(later.current_point < earlier.current_point);
        // No invented point-count/current-point or first/last-time snapshot invariant.
        assert!(decode_state(200, &changed("current_point", i32::MAX.into())).is_ok());
        assert!(decode_state(200, &changed("first_point_time", u32::MAX.into())).is_ok());
        assert!(decode_state(200, &changed("points", u32::MAX.into())).is_ok());
        assert!(decode_state(200, &changed("current_time", i32::MAX.into())).is_ok());
        assert!(decode_state(200, &changed("max_points", u32::MAX.into())).is_ok());
        for (number, expected) in [
            (0, HspPlayState::NotInitialized),
            (1, HspPlayState::Playing),
            (2, HspPlayState::Stopped),
            (3, HspPlayState::Paused),
            (4, HspPlayState::Starving),
        ] {
            assert_eq!(
                decode_state(200, &changed("play_state", number.into()))
                    .unwrap()
                    .play_state,
                expected
            );
        }
    }

    #[test]
    fn preserves_finite_preplay_rates_without_confusing_them_with_play_policy() {
        let fixture = std::str::from_utf8(STATE).unwrap();
        for play_state in [0, 2] {
            for rate in [0.0, -1.0, -125.5] {
                let bytes = fixture
                    .replace("\"play_state\":1", &format!("\"play_state\":{play_state}"))
                    .replace(
                        "\"playback_rate\":1.0",
                        &format!("\"playback_rate\":{rate}"),
                    );
                let observed = decode_state(200, bytes.as_bytes()).unwrap();
                assert_eq!(observed.playback_rate, rate);
                assert_eq!(
                    observed.play_state,
                    if play_state == 0 {
                        HspPlayState::NotInitialized
                    } else {
                        HspPlayState::Stopped
                    }
                );
            }
        }
        for nonfinite in ["NaN", "Infinity", "-Infinity", "1e999", "-1e999"] {
            let bytes = fixture.replace(
                "\"playback_rate\":1.0",
                &format!("\"playback_rate\":{nonfinite}"),
            );
            assert_eq!(
                decode_state(200, bytes.as_bytes()),
                Err(HspWireError::InvalidResponse)
            );
        }
    }

    #[test]
    fn rejects_missing_fields_type_changes_fractional_capacity_and_overflows() {
        let fixture: serde_json::Value = serde_json::from_slice(STATE).unwrap();
        let fields = fixture["result"].as_object().unwrap();
        assert_eq!(fields.len(), 13);
        for field in fields.keys() {
            let mut missing = fixture.clone();
            missing["result"].as_object_mut().unwrap().remove(field);
            assert_eq!(
                decode_state(200, &serde_json::to_vec(&missing).unwrap()),
                Err(HspWireError::InvalidResponse),
                "missing {field}"
            );
            assert_eq!(
                decode_state(200, &changed(field, serde_json::Value::Null)),
                Err(HspWireError::InvalidResponse),
                "null {field}"
            );
            assert_eq!(
                decode_state(200, &changed(field, "secret-looking-peer-value".into())),
                Err(HspWireError::InvalidResponse),
                "string {field}"
            );
        }
        for (field, bad) in [
            ("play_state", serde_json::json!(5)),
            ("play_state", serde_json::json!(1.0)),
            ("play_state", serde_json::json!("HSP_STATE_STARVING")),
            ("max_points", serde_json::json!(2.5)),
            ("max_points", serde_json::json!(-1)),
            ("max_points", serde_json::json!(4294967296_u64)),
            ("current_point", serde_json::json!(-2)),
            ("current_point", serde_json::json!(2147483648_i64)),
            ("current_time", serde_json::json!(-2147483649_i64)),
            ("current_time", serde_json::json!(2147483648_i64)),
            ("current_time", serde_json::json!(0.0)),
            ("tail_point_stream_index", serde_json::json!(-2)),
            ("tail_point_stream_index", serde_json::json!(2147483648_i64)),
        ] {
            assert_eq!(
                decode_state(200, &changed(field, bad)),
                Err(HspWireError::InvalidResponse),
                "invalid {field}"
            );
        }
        for field in [
            "points",
            "first_point_time",
            "last_point_time",
            "stream_id",
            "tail_point_stream_index_threshold",
        ] {
            for bad in [
                serde_json::json!(-1),
                serde_json::json!(1.0),
                serde_json::json!(4294967296_u64),
            ] {
                assert_eq!(
                    decode_state(200, &changed(field, bad)),
                    Err(HspWireError::InvalidResponse),
                    "invalid {field}"
                );
            }
        }
    }

    #[test]
    fn rejects_duplicate_fields_contradictory_envelopes_and_nonfinite_numbers() {
        let fixture = std::str::from_utf8(STATE).unwrap();
        let value: serde_json::Value = serde_json::from_slice(STATE).unwrap();
        for (field, original) in value["result"].as_object().unwrap() {
            let duplicate = fixture.replacen(
                "{\"result\":{",
                &format!("{{\"result\":{{\"{field}\":{original},"),
                1,
            );
            assert_eq!(
                decode_state(200, duplicate.as_bytes()),
                Err(HspWireError::InvalidResponse),
                "duplicate {field}"
            );
        }
        let state = serde_json::to_string(&value["result"]).unwrap();
        for envelope in [
            format!("{{\"result\":{state},\"result\":{state}}}"),
            format!("{{\"result\":{state},\"error\":null}}"),
            format!("{{\"error\":{{\"message\":\"secret://token\"}},\"result\":{state}}}"),
            format!("{{\"result\":{state},\"ok\":true}}"),
            state,
            fixture.replace("100.0", "1e999"),
            fixture.replace("100.0", "NaN"),
            fixture.replace("1.0", "Infinity"),
            fixture.replace(
                "\"current_point\":2",
                "\"current_point\":2,\"peer_token\":\"secret\"",
            ),
        ] {
            assert_eq!(
                decode_state(200, envelope.as_bytes()),
                Err(HspWireError::InvalidResponse)
            );
        }
    }

    #[test]
    fn rejects_transport_and_parse_failures_without_retaining_peer_text() {
        for status in [0, 201, 204, 400, 401, 500] {
            assert_eq!(decode_state(status, STATE), Err(HspWireError::HttpFailure));
        }
        assert_eq!(
            decode_state(200, &vec![b' '; HSP_RESPONSE_LIMIT + 1]),
            Err(HspWireError::OversizedResponse)
        );
        for bytes in [
            b"".as_slice(),
            b"null",
            b"{}",
            b"[]",
            b"{\"result\":null}",
            b"not json: secret://peer-token",
            b"\xff",
        ] {
            let error = decode_state(200, bytes).unwrap_err();
            assert_eq!(error, HspWireError::InvalidResponse);
            assert_eq!(format!("{error:?}"), "InvalidResponse");
        }
        let mut trailing = STATE.to_vec();
        trailing.extend_from_slice(b"{}");
        assert_eq!(
            decode_state(200, &trailing),
            Err(HspWireError::InvalidResponse)
        );
    }
}
