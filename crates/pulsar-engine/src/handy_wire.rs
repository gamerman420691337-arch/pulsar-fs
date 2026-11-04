//! Offline Handy REST v3 codecs. Encoding does not admit or dispatch playback.
//!
//! The owning OpenAPI is linked in handy-integration-research-2026-10-01.md.
//! V2/FW3 and BLE use different contracts. No family fallback is performed.
use pulsar_protocol::HandySetupAcknowledgment;
use serde::{Deserialize, Serialize};

pub const RESPONSE_LIMIT: usize = 64 * 1024;
const MAX_CLOCK_MS: u64 = (1 << 53) - 1;

#[cfg(test)]
mod device_timing_tests {
    use super::*;
    #[test]
    fn handy_device_timing_wire_preserves_signed_integers_and_rejects_ambiguous_or_error_envelopes()
    {
        let info = decode_device_time(
            200,
            br#"{"result":{"time":769799,"clock_offset":-125,"rtd":100}}"#,
        )
        .unwrap();
        assert_eq!(
            (
                info.device_uptime_ms,
                info.reported_clock_offset_ms,
                info.last_sync_round_trip_ms
            ),
            (769799, -125, 100)
        );
        for n in [-125, 0, 125] {
            assert_eq!(
                decode_user_time_offset(
                    200,
                    format!("{{\"result\":{{\"offset\":{n}}}}}").as_bytes()
                )
                .unwrap()
                .offset_ms,
                n
            );
        }
        for bad in [
            r#"{"result":{"time":-1,"clock_offset":0,"rtd":0}}"#,
            r#"{"result":{"time":0,"clock_offset":0,"rtd":-1}}"#,
            r#"{"result":{"time":0,"clock_offset":0.5,"rtd":0}}"#,
            r#"{"result":{"time":0,"clock_offset":0,"rtd":0.0}}"#,
            r#"{"result":{"time":0,"clock_offset":0}}"#,
            r#"{"result":{"time":0,"time":1,"clock_offset":0,"rtd":0}}"#,
            r#"{"result":{"time":0,"clock_offset":9007199254740992,"rtd":0}}"#,
            r#"{"result":{"time":0,"clock_offset":0,"rtd":0},"error":null}"#,
        ] {
            assert_eq!(
                decode_device_time(200, bad.as_bytes()),
                Err(WireError::InvalidResponse)
            );
        }
        for bad in [
            r#"{"result":{"offset":-1.25}}"#,
            r#"{"result":{"offset":1.0}}"#,
            r#"{"result":{"offset":"1"}}"#,
            r#"{"result":{"offset":1,"offset":2}}"#,
            r#"{"result":{"offset":1},"error":{"message":"withheld"}}"#,
            r#"{"result":{"offset":-9007199254740992}}"#,
            r#"{"result":{}}"#,
        ] {
            assert_eq!(
                decode_user_time_offset(200, bad.as_bytes()),
                Err(WireError::InvalidResponse)
            );
        }
        assert_eq!(
            decode_user_time_offset(503, b"secret"),
            Err(WireError::HttpFailure)
        );
        assert_eq!(
            decode_device_time(200, &vec![b' '; RESPONSE_LIMIT + 1]),
            Err(WireError::OversizedResponse)
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireError {
    InvalidClock,
    OversizedResponse,
    HttpFailure,
    InvalidResponse,
}

/// Contains no credentials or configurable URL. A future engine broker must
/// independently require consent, qualification and a live admission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedRequest {
    method: &'static str,
    path: &'static str,
    body: Option<Vec<u8>>,
}

impl EncodedRequest {
    pub fn method(&self) -> &'static str {
        self.method
    }
    pub fn path(&self) -> &'static str {
        self.path
    }
    pub fn body(&self) -> Option<&[u8]> {
        self.body.as_deref()
    }
    pub fn info() -> Self {
        Self {
            method: "GET",
            path: "/info",
            body: None,
        }
    }
    pub fn connected() -> Self {
        Self {
            method: "GET",
            path: "/connected",
            body: None,
        }
    }
    /// Preserves the admitted script's timing: normal rate, no looping.
    /// Caller-supplied milliseconds are not rounded or resynchronized here.
    pub fn hssp_play(start_time_ms: u64, server_time_ms: u64) -> Result<Self, WireError> {
        if start_time_ms > MAX_CLOCK_MS || server_time_ms == 0 || server_time_ms > MAX_CLOCK_MS {
            return Err(WireError::InvalidClock);
        }
        #[derive(Serialize)]
        struct Play {
            start_time: u64,
            server_time: u64,
            playback_rate: f64,
            r#loop: bool,
        }
        let body = serde_json::to_vec(&Play {
            start_time: start_time_ms,
            server_time: server_time_ms,
            playback_rate: 1.0,
            r#loop: false,
        })
        .map_err(|_| WireError::InvalidClock)?;
        Ok(Self {
            method: "PUT",
            path: "/hssp/play",
            body: Some(body),
        })
    }
    pub fn hssp_stop() -> Self {
        Self {
            method: "PUT",
            path: "/hssp/stop",
            body: None,
        }
    }
}

// Reject any error member, including one accompanied by a success result.
// Server messages and arbitrary extension fields never enter diagnostics.
fn reject_error<'de, D: serde::Deserializer<'de>>(_: D) -> Result<(), D::Error> {
    Err(serde::de::Error::custom("remote error"))
}
#[derive(Deserialize)]
struct Envelope<T> {
    result: T,
    #[serde(rename = "error", default, deserialize_with = "reject_error")]
    _error: (),
}

fn result<T: serde::de::DeserializeOwned>(status: u16, bytes: &[u8]) -> Result<T, WireError> {
    if status != 200 {
        return Err(WireError::HttpFailure);
    }
    if bytes.len() > RESPONSE_LIMIT {
        return Err(WireError::OversizedResponse);
    }
    let envelope: Envelope<T> =
        serde_json::from_slice(bytes).map_err(|_| WireError::InvalidResponse)?;
    Ok(envelope.result)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceIdentity {
    pub firmware: String,
    pub model_number: u32,
    pub model_name: String,
    pub model_variant: u32,
    pub session_id: String,
    pub firmware_status: u8,
}

pub fn decode_info(status: u16, bytes: &[u8]) -> Result<DeviceIdentity, WireError> {
    #[derive(Deserialize)]
    struct Info {
        fw_version: String,
        fw_status: u8,
        hw_model_no: u32,
        hw_model_name: String,
        hw_model_variant: u32,
        session_id: String,
    }
    let info: Info = result(status, bytes)?;
    // Missing model identity is not sufficient to bind an exact configuration,
    // even though the vendor schema makes model fields optional.
    let text = |s: &str, limit| {
        !s.is_empty()
            && s.len() <= limit
            && s.bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    };
    if !text(&info.fw_version, 64)
        || !text(&info.hw_model_name, 64)
        || !text(&info.session_id, 128)
        || info.fw_status > 2
    {
        return Err(WireError::InvalidResponse);
    }
    Ok(DeviceIdentity {
        firmware: info.fw_version,
        firmware_status: info.fw_status,
        model_number: info.hw_model_no,
        model_name: info.hw_model_name,
        model_variant: info.hw_model_variant,
        session_id: info.session_id,
    })
}

pub fn decode_connected(status: u16, bytes: &[u8]) -> Result<bool, WireError> {
    #[derive(Deserialize)]
    struct Connected {
        connected: bool,
    }
    Ok(result::<Connected>(status, bytes)?.connected)
}

pub fn decode_device_time(
    status: u16,
    bytes: &[u8],
) -> Result<pulsar_protocol::HandyDeviceTimeInfo, WireError> {
    #[derive(Deserialize)]
    struct Info {
        time: u64,
        clock_offset: i64,
        rtd: u64,
    }
    let raw: Info = result(status, bytes)?;
    let info = pulsar_protocol::HandyDeviceTimeInfo {
        device_uptime_ms: raw.time,
        reported_clock_offset_ms: raw.clock_offset,
        last_sync_round_trip_ms: raw.rtd,
    };
    info.validate().map_err(|_| WireError::InvalidResponse)?;
    Ok(info)
}
pub fn decode_user_time_offset(
    status: u16,
    bytes: &[u8],
) -> Result<pulsar_protocol::HandyUserTimeOffset, WireError> {
    // The GET schema references nonnegative Timestamp but its own examples include
    // -125. Preserve signed integral examples; never round fractional GET values.
    #[derive(Deserialize)]
    struct Offset {
        offset: i64,
    }
    let raw: Offset = result(status, bytes)?;
    let offset = pulsar_protocol::HandyUserTimeOffset {
        offset_ms: raw.offset,
    };
    offset.validate().map_err(|_| WireError::InvalidResponse)?;
    Ok(offset)
}

/// Compatibility names for serializable observations, never Core stop evidence.
pub use pulsar_protocol::{
    HandyHsspObservation as HsspObservation, HandyRemotePlayState as RemotePlayState,
};
pub fn decode_mode(
    status: u16,
    bytes: &[u8],
) -> Result<pulsar_protocol::HandyDeviceMode, WireError> {
    #[derive(Deserialize)]
    struct Mode {
        mode: u8,
        mode_session_id: u64,
    }
    let raw: Mode = result(status, bytes)?;
    let mode = pulsar_protocol::HandyDeviceMode {
        mode: raw.mode,
        mode_session_id: raw.mode_session_id,
    };
    mode.validate().map_err(|_| WireError::InvalidResponse)?;
    Ok(mode)
}

pub fn decode_slider_settings(
    status: u16,
    bytes: &[u8],
) -> Result<pulsar_protocol::HandySliderSettings, WireError> {
    let settings: pulsar_protocol::HandySliderSettings = result(status, bytes)?;
    settings
        .validate()
        .map_err(|_| WireError::InvalidResponse)?;
    Ok(settings)
}
pub fn decode_stroke_settings(
    status: u16,
    bytes: &[u8],
) -> Result<pulsar_protocol::HandyStrokeSettings, WireError> {
    let settings: pulsar_protocol::HandyStrokeSettings = result(status, bytes)?;
    settings
        .validate()
        .map_err(|_| WireError::InvalidResponse)?;
    Ok(settings)
}

pub fn decode_hssp(status: u16, bytes: &[u8]) -> Result<HsspObservation, WireError> {
    #[derive(Deserialize)]
    struct State {
        play_state: u8,
        stream_id: u64,
        current_time: u64,
        playback_rate: f64,
        r#loop: bool,
        pause_on_starving: bool,
    }
    let state: State = result(status, bytes)?;
    let play_state = match state.play_state {
        0 => RemotePlayState::NotInitialized,
        1 => RemotePlayState::Playing,
        2 => RemotePlayState::Stopped,
        3 => RemotePlayState::Paused,
        4 => RemotePlayState::Starving,
        _ => return Err(WireError::InvalidResponse),
    };
    if state.stream_id > MAX_CLOCK_MS
        || state.current_time > MAX_CLOCK_MS
        || !state.playback_rate.is_finite()
        || state.playback_rate <= 0.0
    {
        return Err(WireError::InvalidResponse);
    }
    Ok(HsspObservation {
        play_state,
        stream_id: state.stream_id,
        current_time_ms: state.current_time,
        playback_rate: state.playback_rate,
        looping: state.r#loop,
        pause_on_starving: state.pause_on_starving,
    })
}

/// Bounded setup response observation, not loaded-program evidence.
#[derive(Deserialize, Serialize)]
pub(crate) struct SetupObservation {
    play_state: u8,
    pause_on_starving: bool,
    points: u64,
    max_points: f64,
    current_point: u64,
    current_time: u64,
    r#loop: bool,
    playback_rate: f64,
    first_point_time: u64,
    last_point_time: u64,
    pub(crate) stream_id: u64,
    tail_point_stream_index: u64,
    tail_point_stream_index_threshold: u64,
}
pub(crate) fn decode_setup(
    status: u16,
    bytes: &[u8],
) -> Result<HandySetupAcknowledgment, WireError> {
    let value: SetupObservation = result(status, bytes)?;
    // Decode the vendor envelope first; retain only the declared numeric/boolean fields.
    let observed: HandySetupAcknowledgment = serde_json::from_slice(
        &serde_json::to_vec(&value).map_err(|_| WireError::InvalidResponse)?,
    )
    .map_err(|_| WireError::InvalidResponse)?;
    observed
        .validate()
        .map_err(|_| WireError::InvalidResponse)?;
    Ok(observed)
}

#[cfg(test)]
pub(crate) fn fixture_setup_acknowledgment() -> HandySetupAcknowledgment {
    decode_setup(200, br#"{"result":{"play_state":2,"pause_on_starving":false,"points":2,"max_points":2.0,"current_point":0,"current_time":0,"loop":false,"playback_rate":1.0,"first_point_time":0,"last_point_time":1000,"stream_id":8,"tail_point_stream_index":1,"tail_point_stream_index_threshold":1}}"#).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handy_settings_require_declared_fields_and_refuse_ambiguous_aliases_and_remote_errors() {
        let settings: serde_json::Value = serde_json::from_str(include_str!(
            "../../pulsar-protocol/tests/fixtures/handy-operative-settings.json"
        ))
        .unwrap();
        let bytes = serde_json::to_vec(&serde_json::json!({"result":settings["slider"]})).unwrap();
        let value = decode_slider_settings(200, &bytes).unwrap();
        assert_eq!(value.x_limit_stop, 100.0);
        assert!(decode_slider_settings(500, &bytes).is_err());
        for keep_correct in [false, true] {
            let mut wrong = settings["slider"].clone();
            wrong["x_limit_end"] = serde_json::json!(100.0);
            if !keep_correct {
                wrong.as_object_mut().unwrap().remove("x_limit_stop");
            }
            assert!(decode_slider_settings(
                200,
                &serde_json::to_vec(&serde_json::json!({"result":wrong})).unwrap()
            )
            .is_err());
        }
        assert!(decode_slider_settings(
            200,
            &serde_json::to_vec(&serde_json::json!({"result":settings["slider"],"error":null}))
                .unwrap()
        )
        .is_err());
        assert!(
            decode_stroke_settings(200, br#"{"result":{"min":0,"max":1,"min_absolute":0}}"#)
                .is_err()
        );
        assert!(decode_stroke_settings(
            200,
            br#"{"result":{"min":0,"max":2,"min_absolute":0,"max_absolute":100}}"#
        )
        .is_err());
        assert!(decode_stroke_settings(
            200,
            &serde_json::to_vec(&serde_json::json!({"result":settings["stroke"]})).unwrap()
        )
        .is_ok());
    }
    const INFO: &[u8] = br#"{"result":{"fw_version":"4.0.16","fw_status":0,"hw_model_no":2,"hw_model_name":"H02","hw_model_variant":0,"session_id":"01HYMVKHMPYH1S6WTVD6BM7TQA"}}"#;
    const STOPPED: &[u8] = br#"{"result":{"play_state":2,"stream_id":42,"current_time":9876,"playback_rate":1.0,"loop":false,"pause_on_starving":false}}"#;

    #[test]
    fn play_preserves_exact_v3_milliseconds_and_normal_rate() {
        let request = EncodedRequest::hssp_play(9876, 1_797_112_121_123).unwrap();
        assert_eq!(request.method, "PUT");
        assert_eq!(request.path, "/hssp/play");
        assert_eq!(
            request.body.unwrap(),
            br#"{"start_time":9876,"server_time":1797112121123,"playback_rate":1.0,"loop":false}"#
        );
        assert!(EncodedRequest::hssp_play(0, 1).is_ok());
    }
    #[test]
    fn invalid_or_inexact_clock_is_refused_before_encoding() {
        for (start, server) in [
            (0, 0),
            (MAX_CLOCK_MS + 1, 1),
            (0, MAX_CLOCK_MS + 1),
            (u64::MAX, 1),
        ] {
            assert_eq!(
                EncodedRequest::hssp_play(start, server),
                Err(WireError::InvalidClock)
            );
        }
    }
    #[test]
    fn stop_and_read_operations_are_bodyless_fixed_paths() {
        assert_eq!(
            EncodedRequest::hssp_stop(),
            EncodedRequest {
                method: "PUT",
                path: "/hssp/stop",
                body: None
            }
        );
        assert_eq!(EncodedRequest::info().path, "/info");
        assert_eq!(EncodedRequest::connected().path, "/connected");
        assert!(EncodedRequest::info().body.is_none());
    }
    #[test]
    fn exact_identity_and_remote_stopped_state_are_observations() {
        let info = decode_info(200, INFO).unwrap();
        assert_eq!(info.firmware, "4.0.16");
        assert_eq!(info.model_name, "H02");
        assert_eq!(
            decode_hssp(200, STOPPED).unwrap().play_state,
            RemotePlayState::Stopped
        );
        assert!(decode_connected(200, br#"{"result":{"connected":true}}"#).unwrap());
        assert!(!decode_connected(200, br#"{"result":{"connected":false}}"#).unwrap());
    }
    #[test]
    fn http_and_api_errors_are_redacted_and_never_success() {
        for status in [201, 301, 401, 429, 500] {
            assert_eq!(decode_info(status, INFO), Err(WireError::HttpFailure));
        }
        for body in [
            br#"{"error":{"message":"credential-secret"}}"#.as_slice(),
            br#"{"result":{"connected":true},"error":null}"#,
            br#"{"result":{"connected":true},"error":{}}"#,
        ] {
            assert_eq!(decode_connected(200, body), Err(WireError::InvalidResponse));
        }
    }
    #[test]
    fn bounded_and_malformed_responses_refuse_without_echoing_bytes() {
        assert_eq!(
            decode_info(200, &vec![b'x'; RESPONSE_LIMIT + 1]),
            Err(WireError::OversizedResponse)
        );
        for body in [
            b"null".as_slice(),
            b"{}",
            br#"{"result":null}"#,
            br#"{"result":{"connected":1}}"#,
            br#"{"result":{"connected":true,"connected":false}}"#,
            br#"{"result":{"connected":true},"result":{"connected":false}}"#,
        ] {
            assert_eq!(decode_connected(200, body), Err(WireError::InvalidResponse));
        }
    }
    #[test]
    fn old_api_field_names_do_not_satisfy_v3_response() {
        let old = String::from_utf8(STOPPED.to_vec())
            .unwrap()
            .replace("play_state", "playState")
            .replace("stream_id", "streamId");
        assert_eq!(
            decode_hssp(200, old.as_bytes()),
            Err(WireError::InvalidResponse)
        );
    }
    #[test]
    fn identity_must_be_complete_bounded_and_safe() {
        for (from, to) in [
            ("H02", ""),
            ("H02", "H02\\nsecret"),
            ("H02", "H02 secret"),
            ("\"fw_status\":0", "\"fw_status\":3"),
        ] {
            let body = String::from_utf8(INFO.to_vec()).unwrap().replace(from, to);
            assert_eq!(
                decode_info(200, body.as_bytes()),
                Err(WireError::InvalidResponse)
            );
        }
        let body = String::from_utf8(INFO.to_vec())
            .unwrap()
            .replace("\"hw_model_no\":2,", "");
        assert_eq!(
            decode_info(200, body.as_bytes()),
            Err(WireError::InvalidResponse)
        );
    }
    #[test]
    fn unknown_states_and_invalid_timing_are_not_normalized() {
        for (from, to) in [
            ("\"play_state\":2", "\"play_state\":5"),
            ("\"current_time\":9876", "\"current_time\":-1"),
            ("\"playback_rate\":1.0", "\"playback_rate\":0"),
            ("\"playback_rate\":1.0", "\"playback_rate\":1e999"),
        ] {
            let body = String::from_utf8(STOPPED.to_vec())
                .unwrap()
                .replace(from, to);
            assert_eq!(
                decode_hssp(200, body.as_bytes()),
                Err(WireError::InvalidResponse)
            );
        }
    }
    #[test]
    fn setup_mode_requires_exact_generation_and_discards_extensions() {
        let m = decode_mode(
            200,
            br#"{"result":{"mode":1,"mode_session_id":8,"extension":true}}"#,
        )
        .unwrap();
        assert_eq!(m.mode, 1);
        assert_eq!(m.mode_session_id, 8);
        for bytes in [
            br#"{"result":{"mode":1}}"#.as_slice(),
            br#"{"result":{"mode":10,"mode_session_id":1}}"#,
            br#"{"result":{"mode":1,"mode_session_id":1.5}}"#,
            br#"{"result":{"mode":1,"mode_session_id":9007199254740992}}"#,
            br#"{"result":{"mode":1,"mode_session_id":1},"error":{}}"#,
        ] {
            assert!(decode_mode(200, bytes).is_err());
        }
    }
}
