//! Engine-only, consent-bound GET inspection at the fixed manufacturer origin.
use crate::handy_wire;
use pulsar_protocol::*;
use std::{
    io::Read,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const ORIGIN: &str = "https://www.handyfeeling.com/api/handy-rest/v3";
const DEADLINE: Duration = Duration::from_secs(10);
fn unavailable(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::Unavailable, message)
}

use crate::handy_credentials::Credentials;
use std::sync::Arc;

struct Broker<'a> {
    origin: &'a str,
    credentials: Arc<Credentials>,
    deadline: Instant,
}
impl Broker<'_> {
    fn get(&self, path: &str) -> Result<Vec<u8>, ProtocolError> {
        // No arbitrary operation, path, upload or actuation enters this broker.
        if !matches!(
            path,
            "/connected"
                | "/info"
                | "/mode"
                | "/hssp/state"
                | "/hsp/state"
                | "/settings/slider"
                | "/slider/stroke"
                | "/hstp/info"
                | "/hstp/offset"
        ) {
            return Err(ProtocolError::unsupported("Handy inspection operation"));
        }
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(unavailable("Handy inspection deadline expired"));
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(remaining))
            .max_redirects(0)
            .proxy(None)
            .build()
            .into();
        let mut request = agent
            .get(format!("{}{path}", self.origin))
            .header("X-Api-Key", &self.credentials.api)
            .header("X-Connection-Key", &self.credentials.connection)
            .header("Accept", "application/json");
        for name in ["X-Api-Key", "X-Connection-Key"] {
            request
                .headers_mut()
                .and_then(|h| h.get_mut(name))
                .ok_or_else(|| unavailable("Handy authentication header unavailable"))?
                .set_sensitive(true);
        }
        let mut response = request.call().map_err(|_| {
            unavailable("Handy inspection request failed; remote detail is withheld")
        })?;
        if response.status().as_u16() != 200 {
            return Err(unavailable(
                "Handy inspection returned a non-success HTTP status",
            ));
        }
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(handy_wire::RESPONSE_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| unavailable("Handy inspection response read failed"))?;
        if bytes.len() > handy_wire::RESPONSE_LIMIT {
            return Err(unavailable("Handy inspection response exceeds 64 KiB"));
        }
        if Instant::now() >= self.deadline {
            return Err(unavailable("Handy inspection deadline expired"));
        }
        Ok(bytes)
    }
    fn inspect(&self) -> Result<HandyInspection, ProtocolError> {
        let bytes = self.get(handy_wire::EncodedRequest::connected().path())?;
        let connected = handy_wire::decode_connected(200, &bytes)
            .map_err(|_| unavailable("invalid Handy connectivity response"))?;
        let identity = if connected {
            let bytes = self.get(handy_wire::EncodedRequest::info().path())?;
            let raw = handy_wire::decode_info(200, &bytes)
                .map_err(|_| unavailable("invalid Handy identity response"))?;
            let identity = HandyIdentity {
                firmware: raw.firmware,
                model_number: raw.model_number,
                model_name: raw.model_name,
                model_variant: raw.model_variant,
                session_id: raw.session_id,
                firmware_status: raw.firmware_status,
            };
            if self.credentials.echoed(&identity) {
                return Err(unavailable(
                    "credential-bearing Handy identity was withheld",
                ));
            }
            Some(identity)
        } else {
            None
        };
        let report = HandyInspection {
            connected,
            identity,
            observed_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| unavailable("host observation clock is unavailable"))?
                .as_millis()
                .try_into()
                .map_err(|_| unavailable("host observation clock exceeds supported range"))?,
        };
        report.validate()?;
        Ok(report)
    }
}

pub(crate) fn inspect(request: &HandyInspectionRequest) -> Result<HandyInspection, ProtocolError> {
    let credentials = Arc::new(Credentials::from_environment(request)?);
    inspect_credentials(credentials)
}

pub(crate) fn inspect_credentials(
    credentials: Arc<Credentials>,
) -> Result<HandyInspection, ProtocolError> {
    Broker {
        origin: ORIGIN,
        credentials,
        deadline: Instant::now() + DEADLINE,
    }
    .inspect()
}

fn setup_sample(broker: &Broker<'_>) -> Result<HandySetupDeviceObservation, ProtocolError> {
    sample_with_buffer(broker, false).map(|(sample, _)| sample)
}
fn sample_with_buffer(
    broker: &Broker<'_>,
    complete_buffer: bool,
) -> Result<(HandySetupDeviceObservation, Option<HandyHspBuffer>), ProtocolError> {
    let inspected = broker.inspect()?;
    let identity = inspected
        .identity
        .ok_or_else(|| unavailable("Handy is not connected"))?;
    let mode = handy_wire::decode_mode(200, &broker.get("/mode")?)
        .map_err(|_| unavailable("Handy mode generation is unavailable or invalid"))?;
    let operative_settings = HandyOperativeSettings {
        slider: handy_wire::decode_slider_settings(200, &broker.get("/settings/slider")?)
            .map_err(|_| unavailable("invalid Handy slider settings"))?,
        stroke: handy_wire::decode_stroke_settings(200, &broker.get("/slider/stroke")?)
            .map_err(|_| unavailable("invalid Handy stroke settings"))?,
    };
    let mut buffer = None;
    let remote_hssp = if mode.mode == 1 {
        let bytes = broker.get("/hssp/state")?;
        if complete_buffer {
            buffer = Some(
                handy_wire::decode_setup(200, &bytes)
                    .map_err(|_| unavailable("invalid stopped HSP buffer observation"))?,
            );
        }
        Some(
            handy_wire::decode_hssp(200, &bytes)
                .map_err(|_| unavailable("invalid Handy HSSP observation"))?,
        )
    } else {
        None
    };
    let remote_hsp = if mode.mode == 4 {
        let state = crate::handy_hsp_wire::decode_state(200, &broker.get("/hsp/state")?)
            .map_err(|_| unavailable("invalid Handy HSP observation"))?;
        use crate::handy_hsp_wire::HspPlayState;
        Some(HandyHspObservation {
            play_state: match state.play_state {
                HspPlayState::NotInitialized => HandyRemotePlayState::NotInitialized,
                HspPlayState::Playing => HandyRemotePlayState::Playing,
                HspPlayState::Stopped => HandyRemotePlayState::Stopped,
                HspPlayState::Paused => HandyRemotePlayState::Paused,
                HspPlayState::Starving => HandyRemotePlayState::Starving,
            },
            stream_id: state.stream_id,
            current_time_ms: state.current_time,
            points: state.points,
            max_points: state.max_points,
            current_point: state.current_point,
            playback_rate: state.playback_rate,
            looping: state.looping,
            pause_on_starving: state.pause_on_starving,
            first_point_time_ms: state.first_point_time,
            last_point_time_ms: state.last_point_time,
            tail_point_stream_index: state.tail_point_stream_index,
            tail_point_stream_index_threshold: state.tail_point_stream_index_threshold,
        })
    } else {
        None
    };
    let sample = HandySetupDeviceObservation {
        identity,
        operative_settings,
        mode,
        remote_hssp,
        remote_hsp,
        observed_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| unavailable("host clock unavailable"))?
            .as_millis()
            .try_into()
            .map_err(|_| unavailable("host clock overflow"))?,
    };
    sample.validate()?;
    let encoded = serde_json::to_string(&sample)
        .map_err(|_| unavailable("Handy observation encoding failed"))?;
    if broker.credentials.echoed_text(&encoded) {
        return Err(unavailable(
            "credential-bearing Handy observation was withheld",
        ));
    }
    if let Some(value) = &buffer {
        let text = serde_json::to_string(value)
            .map_err(|_| unavailable("buffer observation encoding failed"))?;
        if broker.credentials.echoed_text(&text) {
            return Err(unavailable("credential-bearing HSP buffer was withheld"));
        }
    }
    Ok((sample, buffer))
}
pub(crate) fn inspect_program(
    credentials: Arc<Credentials>,
    deadline: Instant,
) -> Result<(HandySetupDeviceObservation, HandyHspBuffer), ProtocolError> {
    let (sample, buffer) = sample_with_buffer(
        &Broker {
            origin: ORIGIN,
            credentials,
            deadline,
        },
        true,
    )?;
    let buffer = buffer.ok_or_else(|| unavailable("program correlation requires HSSP mode"))?;
    HandyProgramCorrelation::check_buffer(&sample, &buffer)?;
    Ok((sample, buffer))
}
pub(crate) fn inspect_setup(
    credentials: Arc<Credentials>,
    deadline: Instant,
) -> Result<HandySetupDeviceObservation, ProtocolError> {
    setup_sample(&Broker {
        origin: ORIGIN,
        credentials,
        deadline,
    })
}

#[derive(Clone)]
pub(crate) struct DeviceTimingRead {
    pub(crate) before: HandySetupDeviceObservation,
    pub(crate) first: HandyDeviceTimingSample,
    pub(crate) second: HandyDeviceTimingSample,
    pub(crate) after: HandySetupDeviceObservation,
}
#[derive(Clone)]
pub(crate) struct PlayingTimingRead {
    pub(crate) device: DeviceTimingRead,
    pub(crate) started_boottime_ns: u64,
    pub(crate) finished_boottime_ns: u64,
}
fn timing_sample(
    broker: &Broker<'_>,
    clock: &mut dyn FnMut() -> Result<u64, ProtocolError>,
) -> Result<HandyDeviceTimingSample, ProtocolError> {
    let read_started_boottime_ns = clock()?;
    let info = handy_wire::decode_device_time(200, &broker.get("/hstp/info")?)
        .map_err(|_| unavailable("invalid device time observation"))?;
    let user_offset = handy_wire::decode_user_time_offset(200, &broker.get("/hstp/offset")?)
        .map_err(|_| unavailable("invalid user time offset observation"))?;
    let sample = HandyDeviceTimingSample {
        info,
        user_offset,
        read_started_boottime_ns,
        read_finished_boottime_ns: clock()?,
    };
    sample.validate()?;
    let text =
        serde_json::to_string(&sample).map_err(|_| unavailable("device timing encoding failed"))?;
    if broker.credentials.echoed_text(&text) {
        return Err(unavailable("credential-bearing device timing was withheld"));
    }
    Ok(sample)
}
fn device_timing_read(
    broker: &Broker<'_>,
    clock: &mut dyn FnMut() -> Result<u64, ProtocolError>,
) -> Result<DeviceTimingRead, ProtocolError> {
    let read = raw_timing_read(broker, clock)?;
    if !read.device.before.same_state(&read.device.after)
        || read.device.second.info.device_uptime_ms < read.device.first.info.device_uptime_ms
    {
        return Err(unavailable(
            "device context or uptime changed during timing inspection",
        ));
    }
    Ok(read.device)
}
fn raw_timing_read(
    broker: &Broker<'_>,
    clock: &mut dyn FnMut() -> Result<u64, ProtocolError>,
) -> Result<PlayingTimingRead, ProtocolError> {
    let started_boottime_ns = clock()?;
    let before = setup_sample(broker)?;
    let first = timing_sample(broker, clock)?;
    let second = timing_sample(broker, clock)?;
    let after = setup_sample(broker)?;
    let finished_boottime_ns = clock()?;
    if started_boottime_ns > first.read_started_boottime_ns
        || first.read_finished_boottime_ns > second.read_started_boottime_ns
        || second.read_finished_boottime_ns > finished_boottime_ns
    {
        return Err(unavailable("device inspection clock reversed"));
    }
    Ok(PlayingTimingRead {
        device: DeviceTimingRead {
            before,
            first,
            second,
            after,
        },
        started_boottime_ns,
        finished_boottime_ns,
    })
}
fn playing_timing_read(
    broker: &Broker<'_>,
    clock: &mut dyn FnMut() -> Result<u64, ProtocolError>,
) -> Result<PlayingTimingRead, ProtocolError> {
    let read = raw_timing_read(broker, clock)?;
    validate_handy_playing_context(&read.device.before, &read.device.after)?;
    if read.device.second.info.device_uptime_ms < read.device.first.info.device_uptime_ms {
        return Err(unavailable(
            "device uptime reversed during Playing inspection",
        ));
    }
    Ok(read)
}
pub(crate) fn inspect_playing_timing(
    credentials: Arc<Credentials>,
    deadline: Instant,
    clock: &mut dyn FnMut() -> Result<u64, ProtocolError>,
) -> Result<PlayingTimingRead, ProtocolError> {
    playing_timing_read(
        &Broker {
            origin: ORIGIN,
            credentials,
            deadline,
        },
        clock,
    )
}
pub(crate) fn inspect_device_timing(
    credentials: Arc<Credentials>,
    deadline: Instant,
    clock: &mut dyn FnMut() -> Result<u64, ProtocolError>,
) -> Result<DeviceTimingRead, ProtocolError> {
    device_timing_read(
        &Broker {
            origin: ORIGIN,
            credentials,
            deadline,
        },
        clock,
    )
}

/// HSP stream context inspection does not interpret evolving ring counters as
/// an independent phase clock. Only stable identity/settings and the exact
/// owned stream/configuration may carry the original synchronization forward.
pub(crate) fn validate_hsp_clock_context(
    read: &PlayingTimingRead,
    stream_id: u32,
) -> Result<(), ProtocolError> {
    let a = &read.device.before;
    let b = &read.device.after;
    a.validate()?;
    b.validate()?;
    read.device.first.validate()?;
    read.device.second.validate()?;
    if stream_id == 0
        || a.identity != b.identity
        || a.mode != b.mode
        || a.mode.mode != 4
        || a.operative_settings != b.operative_settings
        || read.started_boottime_ns > read.device.first.read_started_boottime_ns
        || read.device.first.read_finished_boottime_ns > read.device.second.read_started_boottime_ns
        || read.device.second.read_finished_boottime_ns > read.finished_boottime_ns
        || read.device.second.info.device_uptime_ms < read.device.first.info.device_uptime_ms
        || read.device.first.user_offset != read.device.second.user_offset
    {
        return Err(unavailable("HSP clock inspection context changed"));
    }
    for observation in [a, b] {
        if observation.remote_hsp.as_ref().is_none_or(|state| {
            state.stream_id != stream_id
                || !matches!(
                    state.play_state,
                    HandyRemotePlayState::Stopped | HandyRemotePlayState::Playing
                )
                || state.playback_rate != 1.0
                || state.looping
                || state.pause_on_starving
        }) {
            return Err(unavailable(
                "HSP clock inspection belongs to another stream or configuration",
            ));
        }
    }
    if a.remote_hsp.as_ref().unwrap().play_state == HandyRemotePlayState::Playing
        && b.remote_hsp.as_ref().unwrap().play_state != HandyRemotePlayState::Playing
    {
        return Err(unavailable("HSP playback retired during clock inspection"));
    }
    if a.remote_hsp.as_ref().unwrap().max_points != b.remote_hsp.as_ref().unwrap().max_points {
        return Err(unavailable("HSP capacity changed during clock inspection"));
    }
    Ok(())
}
pub(crate) fn inspect_hsp_clock_context(
    credentials: Arc<Credentials>,
    deadline: Instant,
    clock: &mut dyn FnMut() -> Result<u64, ProtocolError>,
    stream_id: u32,
) -> Result<PlayingTimingRead, ProtocolError> {
    let read = raw_timing_read(
        &Broker {
            origin: ORIGIN,
            credentials,
            deadline,
        },
        clock,
    )?;
    validate_hsp_clock_context(&read, stream_id)?;
    Ok(read)
}

/// Two bounded, read-only samples; no claim of an atomic remote transaction.
fn qualification_sample(broker: &Broker<'_>) -> Result<HandySetupDeviceObservation, ProtocolError> {
    let first = setup_sample(broker)?;
    let second = setup_sample(broker)?;
    if !first.same_state(&second) {
        return Err(unavailable(
            "Handy configuration changed during qualification inspection",
        ));
    }
    Ok(second)
}
pub(crate) fn inspect_qualification(
    credentials: Arc<Credentials>,
) -> Result<HandySetupDeviceObservation, ProtocolError> {
    qualification_sample(&Broker {
        origin: ORIGIN,
        credentials,
        deadline: Instant::now() + Duration::from_secs(30),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        thread,
    };
    fn accept(listener: &TcpListener) -> std::net::TcpStream {
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match listener.accept() {
                Ok((socket, _)) => return socket,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "fixture connection deadline");
                    thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("fixture accept failed: {e}"),
            }
        }
    }
    fn mock(responses: Vec<(u16, String)>) -> (String, thread::JoinHandle<Vec<String>>) {
        mock_with_keys(responses, "fixture-api", "fixture-device")
    }
    fn mock_with_keys(
        responses: Vec<(u16, String)>,
        expected_api: &str,
        expected_connection: &str,
    ) -> (String, thread::JoinHandle<Vec<String>>) {
        let expected_api = expected_api.to_string();
        let expected_connection = expected_connection.to_string();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let join = thread::spawn(move || {
            let mut paths = vec![];
            for (status, body) in responses {
                let mut socket = accept(&listener);
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut reader = BufReader::new(socket.try_clone().unwrap());
                let mut line = String::new();
                assert!(
                    reader.read_line(&mut line).unwrap() > 0,
                    "fixture request ended early"
                );
                assert!(line.starts_with("GET "));
                paths.push(line.trim().to_string());
                let mut headers = String::new();
                loop {
                    line.clear();
                    assert!(
                        reader.read_line(&mut line).unwrap() > 0,
                        "fixture headers ended early"
                    );
                    if line == "\r\n" {
                        break;
                    }
                    headers.push_str(&line);
                }
                assert!(headers
                    .to_ascii_lowercase()
                    .contains(&format!("x-api-key: {expected_api}")));
                assert!(headers
                    .to_ascii_lowercase()
                    .contains(&format!("x-connection-key: {expected_connection}")));
                write!(socket,"HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
            paths
        });
        (origin, join)
    }
    fn broker(origin: &str) -> Broker<'_> {
        Broker {
            origin,
            credentials: Arc::new(Credentials {
                api: "fixture-api".into(),
                connection: "fixture-device".into(),
            }),
            deadline: Instant::now() + Duration::from_secs(3),
        }
    }
    const INFO: &str = r#"{"result":{"fw_version":"4.0.16","fw_status":0,"hw_model_no":2,"hw_model_name":"H02","hw_model_variant":0,"session_id":"session-fixture"}}"#;
    #[test]
    fn inspector_issues_only_connected_and_identity_gets() {
        let (origin, join) = mock(vec![
            (200, r#"{"result":{"connected":true}}"#.into()),
            (200, INFO.into()),
        ]);
        let result = broker(&origin).inspect().unwrap();
        assert_eq!(result.identity.unwrap().model_name, "H02");
        assert_eq!(
            join.join().unwrap(),
            ["GET /connected HTTP/1.1", "GET /info HTTP/1.1"]
        );
    }
    #[test]
    fn disconnected_inspection_does_not_query_info() {
        let (origin, join) = mock(vec![(200, r#"{"result":{"connected":false}}"#.into())]);
        let result = broker(&origin).inspect().unwrap();
        assert!(!result.connected);
        assert!(result.identity.is_none());
        assert_eq!(join.join().unwrap().len(), 1);
    }
    #[test]
    fn api_and_http_errors_are_withheld() {
        for (status, body) in [
            (401, "credential-secret"),
            (429, "credential-secret"),
            (200, r#"{"error":{"message":"credential-secret"}}"#),
        ] {
            let (origin, join) = mock(vec![(status, body.into())]);
            let error = broker(&origin).inspect().unwrap_err();
            assert!(!format!("{error:?}").contains("credential-secret"));
            join.join().unwrap();
        }
    }
    #[test]
    fn identity_cannot_echo_credentials() {
        let (origin, join) = mock(vec![
            (200, r#"{"result":{"connected":true}}"#.into()),
            (200, INFO.replace("session-fixture", "fixture-device")),
        ]);
        assert!(broker(&origin).inspect().is_err());
        join.join().unwrap();
    }
    #[test]
    fn expired_deadline_and_unapproved_path_make_no_connection() {
        let mut broker = broker("http://127.0.0.1:1");
        assert_eq!(
            broker.get("/hssp/play").unwrap_err().code,
            ErrorCode::Unsupported
        );
        broker.deadline = Instant::now();
        assert!(broker.inspect().is_err());
    }
    #[test]
    fn consent_is_checked_before_credentials_or_network() {
        let r = HandyInspectionRequest {
            api_key_env: "HANDY_MISSING_FIXTURE_API".into(),
            connection_key_env: "HANDY_MISSING_FIXTURE_CONNECTION".into(),
            allow_online_inspection: false,
        };
        assert_eq!(inspect(&r).unwrap_err().code, ErrorCode::Forbidden);
        for key in ["", "bad\r\nheader", "bad value"] {
            assert!(Credentials {
                api: key.into(),
                connection: "fixture".into()
            }
            .validate()
            .is_err());
        }
    }
    #[test]
    fn oversized_http_body_is_refused_before_decoding() {
        let (origin, join) = mock(vec![(200, "x".repeat(handy_wire::RESPONSE_LIMIT + 1))]);
        let error = broker(&origin).inspect().unwrap_err();
        assert!(error.message.contains("exceeds 64 KiB"));
        join.join().unwrap();
    }
    #[test]
    fn redirect_cannot_forward_credentials_to_another_origin() {
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let source = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", source.local_addr().unwrap());
        let location = format!("http://{}/info", target.local_addr().unwrap());
        let join = thread::spawn(move || {
            let mut socket = accept(&source);
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut reader = BufReader::new(socket.try_clone().unwrap());
            let mut line = String::new();
            loop {
                line.clear();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
            }
            write!(socket,"HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        assert!(broker(&origin).inspect().is_err());
        join.join().unwrap();
        assert_eq!(
            target.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
    #[test]
    fn late_response_cannot_outlive_global_inspection_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let join = thread::spawn(move || {
            let mut socket = accept(&listener);
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut reader = BufReader::new(socket.try_clone().unwrap());
            let mut line = String::new();
            loop {
                line.clear();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
            }
            thread::sleep(Duration::from_millis(100));
            let body = r#"{"result":{"connected":false}}"#;
            let _ = write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        });
        let mut broker = broker(&origin);
        broker.deadline = Instant::now() + Duration::from_millis(50);
        assert!(broker.inspect().is_err());
        join.join().unwrap();
    }
    fn setup_responses(mode: u8, settings: &HandyOperativeSettings) -> Vec<(u16, String)> {
        let mut responses = vec![
            (200, r#"{"result":{"connected":true}}"#.into()),
            (200, INFO.into()),
            (
                200,
                format!(r#"{{"result":{{"mode":{mode},"mode_session_id":42}}}}"#),
            ),
            (
                200,
                serde_json::json!({"result":settings.slider}).to_string(),
            ),
            (
                200,
                serde_json::json!({"result":settings.stroke}).to_string(),
            ),
        ];
        if mode == 1 {
            responses.push((200,r#"{"result":{"play_state":2,"stream_id":7,"current_time":0,"playback_rate":1.0,"loop":false,"pause_on_starving":false}}"#.into()));
        }
        responses
    }
    fn settings() -> HandyOperativeSettings {
        serde_json::from_str(include_str!(
            "../../pulsar-protocol/tests/fixtures/handy-operative-settings.json"
        ))
        .unwrap()
    }
    const PLAYING_TIMING_PATHS: [&str; 16] = [
        "GET /connected HTTP/1.1",
        "GET /info HTTP/1.1",
        "GET /mode HTTP/1.1",
        "GET /settings/slider HTTP/1.1",
        "GET /slider/stroke HTTP/1.1",
        "GET /hssp/state HTTP/1.1",
        "GET /hstp/info HTTP/1.1",
        "GET /hstp/offset HTTP/1.1",
        "GET /hstp/info HTTP/1.1",
        "GET /hstp/offset HTTP/1.1",
        "GET /connected HTTP/1.1",
        "GET /info HTTP/1.1",
        "GET /mode HTTP/1.1",
        "GET /settings/slider HTTP/1.1",
        "GET /slider/stroke HTTP/1.1",
        "GET /hssp/state HTTP/1.1",
    ];
    fn change_response(
        responses: &mut [(u16, String)],
        index: usize,
        field: &str,
        value: serde_json::Value,
    ) {
        let mut response: serde_json::Value = serde_json::from_str(&responses[index].1).unwrap();
        response["result"][field] = value;
        responses[index].1 = response.to_string();
    }
    fn playing_responses() -> Vec<(u16, String)> {
        let mut responses = setup_responses(1, &settings());
        responses.extend([
            (
                200,
                r#"{"result":{"time":100,"clock_offset":-125,"rtd":10}}"#.into(),
            ),
            (200, r#"{"result":{"offset":-50}}"#.into()),
            (
                200,
                r#"{"result":{"time":101,"clock_offset":126,"rtd":20}}"#.into(),
            ),
            (200, r#"{"result":{"offset":50}}"#.into()),
        ]);
        responses.extend(setup_responses(1, &settings()));
        for (index, phase) in [(5, 100), (15, 102)] {
            change_response(&mut responses, index, "play_state", serde_json::json!(1));
            change_response(
                &mut responses,
                index,
                "current_time",
                serde_json::json!(phase),
            );
        }
        responses
    }
    #[test]
    fn handy_playing_timing_reads_exactly_sixteen_gets_and_accepts_advancing_playtime() {
        let (origin, join) = mock(playing_responses());
        let mut times = [100, 101, 102, 103, 104, 105].into_iter();
        let result = playing_timing_read(&broker(&origin), &mut || Ok(times.next().unwrap()));
        let paths = join.join().unwrap();
        assert_eq!(paths, PLAYING_TIMING_PATHS);
        let read = result.unwrap();
        assert!(times.next().is_none());
        assert_eq!(read.started_boottime_ns, 100);
        assert_eq!(read.finished_boottime_ns, 105);
        assert_eq!(read.device.first.read_started_boottime_ns, 101);
        assert_eq!(read.device.first.read_finished_boottime_ns, 102);
        assert_eq!(read.device.second.read_started_boottime_ns, 103);
        assert_eq!(read.device.second.read_finished_boottime_ns, 104);
        assert!(!read.device.before.same_state(&read.device.after));
        let before = read.device.before.remote_hssp.unwrap();
        let after = read.device.after.remote_hssp.unwrap();
        assert_eq!(before.play_state, HandyRemotePlayState::Playing);
        assert_eq!(after.play_state, HandyRemotePlayState::Playing);
        assert_eq!((before.current_time_ms, after.current_time_ms), (100, 102));
        // These raw HSTP values remain observations, not a scheduler equation.
        assert_eq!(read.device.first.info.reported_clock_offset_ms, -125);
        assert_eq!(read.device.second.info.reported_clock_offset_ms, 126);
        assert_eq!(read.device.first.user_offset.offset_ms, -50);
        assert_eq!(read.device.second.user_offset.offset_ms, 50);
    }
    #[test]
    fn handy_preplay_timing_still_refuses_advancing_playing_state() {
        let (origin, join) = mock(playing_responses());
        let mut times = [100, 101, 102, 103, 104, 105].into_iter();
        let result = device_timing_read(&broker(&origin), &mut || Ok(times.next().unwrap()));
        assert_eq!(join.join().unwrap(), PLAYING_TIMING_PATHS);
        let error = result.err().expect("preplay context must remain unchanged");
        assert_eq!(
            error.message,
            "device context or uptime changed during timing inspection"
        );
    }
    #[test]
    fn handy_playing_timing_refuses_changed_context_or_reversed_reported_time() {
        for (name, index, field, value) in [
            ("identity", 11, "fw_version", serde_json::json!("4.0.17")),
            (
                "remote session",
                11,
                "session_id",
                serde_json::json!("other-session"),
            ),
            ("mode", 12, "mode", serde_json::json!(7)),
            ("mode session", 12, "mode_session_id", serde_json::json!(43)),
            (
                "slider settings",
                13,
                "overclock_enabled",
                serde_json::json!(true),
            ),
            ("stroke settings", 14, "max", serde_json::json!(0.9)),
            ("stream", 15, "stream_id", serde_json::json!(8)),
            ("before rate", 5, "playback_rate", serde_json::json!(0.5)),
            ("after rate", 15, "playback_rate", serde_json::json!(1.5)),
            ("loop", 15, "loop", serde_json::json!(true)),
            (
                "pause on starving",
                15,
                "pause_on_starving",
                serde_json::json!(true),
            ),
            ("paused before", 5, "play_state", serde_json::json!(3)),
            ("paused after", 15, "play_state", serde_json::json!(3)),
            ("stopped", 15, "play_state", serde_json::json!(2)),
            ("starving", 15, "play_state", serde_json::json!(4)),
            ("reversed phase", 15, "current_time", serde_json::json!(99)),
        ] {
            let mut responses = playing_responses();
            change_response(&mut responses, index, field, value);
            if name == "mode" {
                responses.pop(); // The non-HSSP mode correctly omits /hssp/state.
            }
            let expected_requests = responses.len();
            let (origin, join) = mock(responses);
            let mut times = [100, 101, 102, 103, 104, 105].into_iter();
            let result = playing_timing_read(&broker(&origin), &mut || Ok(times.next().unwrap()));
            let paths = join.join().unwrap();
            assert_eq!(paths, PLAYING_TIMING_PATHS[..expected_requests], "{name}");
            let error = result
                .err()
                .unwrap_or_else(|| panic!("accepted changed {name}"));
            let expected = if name == "mode" {
                "continuous timing requires HSSP observations"
            } else {
                "continuous timing playing context changed"
            };
            assert_eq!(error.message, expected, "{name}");
        }
    }
    #[test]
    fn handy_playing_timing_refuses_every_clock_reversal_and_uptime_regression() {
        for (times, requests, message) in [
            (
                [102, 101, 102, 103, 104, 105],
                16,
                "device inspection clock reversed",
            ),
            (
                [100, 101, 100, 103, 104, 105],
                8,
                "device timing observation clock reversed",
            ),
            (
                [100, 101, 102, 101, 104, 105],
                16,
                "device inspection clock reversed",
            ),
            (
                [100, 101, 102, 103, 102, 105],
                10,
                "device timing observation clock reversed",
            ),
            (
                [100, 101, 102, 103, 104, 103],
                16,
                "device inspection clock reversed",
            ),
        ] {
            let mut responses = playing_responses();
            responses.truncate(requests);
            let (origin, join) = mock(responses);
            let mut times = times.into_iter();
            let result = playing_timing_read(&broker(&origin), &mut || Ok(times.next().unwrap()));
            assert_eq!(join.join().unwrap(), PLAYING_TIMING_PATHS[..requests]);
            assert_eq!(
                result.err().expect("clock reversal must fail").message,
                message
            );
        }
        let mut responses = playing_responses();
        change_response(&mut responses, 8, "time", serde_json::json!(99));
        let (origin, join) = mock(responses);
        let mut times = [100, 101, 102, 103, 104, 105].into_iter();
        let result = playing_timing_read(&broker(&origin), &mut || Ok(times.next().unwrap()));
        assert_eq!(join.join().unwrap(), PLAYING_TIMING_PATHS);
        assert_eq!(
            result.err().expect("uptime regression must fail").message,
            "device uptime reversed during Playing inspection"
        );
    }
    #[test]
    fn handy_playing_inspection_broker_refuses_sync_and_effect_routes_before_network() {
        for path in [
            "/hstp/clocksync",
            "/hstp/clocksync?s=true",
            "/hstp/synctime",
            "/hssp/synctime",
            "/hstp/offset?offset=0",
            "/hssp/setup",
            "/hssp/play",
            "/hssp/stop",
            "/mode?mode=1",
        ] {
            let error = broker("http://127.0.0.1:1").get(path).unwrap_err();
            assert_eq!(error.code, ErrorCode::Unsupported, "{path}");
        }
    }
    #[test]
    fn handy_device_timing_online_reads_only_info_and_offset_between_bound_device_samples() {
        for case in 0..3 {
            let mut responses = setup_responses(1, &settings());
            responses.extend([
                (
                    200,
                    r#"{"result":{"time":100,"clock_offset":-125,"rtd":10}}"#.into(),
                ),
                (200, r#"{"result":{"offset":-50}}"#.into()),
                (
                    200,
                    format!(
                        r#"{{"result":{{"time":{},"clock_offset":126,"rtd":20}}}}"#,
                        if case == 2 { 99 } else { 101 }
                    ),
                ),
                (200, r#"{"result":{"offset":50}}"#.into()),
            ]);
            let mut last = setup_responses(1, &settings());
            if case == 1 {
                last[1].1 = INFO.replace("session-fixture", "changed-session");
            }
            responses.extend(last);
            let (origin, join) = mock(responses);
            let mut tick = 100;
            let result = device_timing_read(&broker(&origin), &mut || {
                tick += 1;
                Ok(tick)
            });
            assert_eq!(result.is_ok(), case == 0);
            if let Ok(r) = result {
                assert_eq!(r.first.info.reported_clock_offset_ms, -125);
                assert_eq!(r.first.user_offset.offset_ms, -50);
                assert_eq!(r.second.user_offset.offset_ms, 50);
                assert!(r.first.read_finished_boottime_ns <= r.second.read_started_boottime_ns);
            }
            let paths = join.join().unwrap();
            assert_eq!(paths.len(), 16);
            assert_eq!(
                &paths[6..10],
                [
                    "GET /hstp/info HTTP/1.1",
                    "GET /hstp/offset HTTP/1.1",
                    "GET /hstp/info HTTP/1.1",
                    "GET /hstp/offset HTTP/1.1"
                ]
            );
            assert!(!paths.iter().any(|p| p.contains("clocksync")));
        }
    }
    #[test]
    fn handy_device_timing_online_withholds_numeric_echo_and_refuses_clock_sync_path() {
        let (origin, join) = mock_with_keys(
            vec![
                (
                    200,
                    r#"{"result":{"time":100,"clock_offset":-42424242,"rtd":10}}"#.into(),
                ),
                (200, r#"{"result":{"offset":0}}"#.into()),
            ],
            "fixture-api",
            "42424242",
        );
        let b = Broker {
            origin: &origin,
            credentials: Arc::new(Credentials {
                api: "fixture-api".into(),
                connection: "42424242".into(),
            }),
            deadline: Instant::now() + Duration::from_secs(3),
        };
        let error = timing_sample(&b, &mut || Ok(1)).unwrap_err();
        assert!(error.message.contains("withheld"));
        assert!(!error.message.contains("42424242"));
        assert_eq!(join.join().unwrap().len(), 2);
        for path in [
            "/hstp/clocksync",
            "/hstp/clocksync?s=true",
            "/hstp/offset?offset=0",
        ] {
            assert!(broker("http://127.0.0.1:1").get(path).is_err());
        }
    }
    #[test]
    fn handy_program_sample_decodes_complete_buffer_from_the_same_read_and_withholds_echoes() {
        for echo in [false, true] {
            let mut responses = setup_responses(1, &settings());
            let mut value = crate::handy_wire::fixture_setup_acknowledgment();
            if echo {
                value.tail_point_stream_index = 42424242;
            }
            responses.last_mut().unwrap().1 = serde_json::json!({"result":value}).to_string();
            let key = if echo { "42424242" } else { "fixture-device" };
            let (origin, join) = mock_with_keys(responses, "fixture-api", key);
            let broker = Broker {
                origin: &origin,
                credentials: Arc::new(Credentials {
                    api: "fixture-api".into(),
                    connection: key.into(),
                }),
                deadline: Instant::now() + Duration::from_secs(3),
            };
            let result = sample_with_buffer(&broker, true);
            if echo {
                assert!(result.unwrap_err().message.contains("withheld"));
            } else {
                let (sample, buffer) = result.unwrap();
                let buffer = buffer.unwrap();
                HandyProgramCorrelation::check_buffer(&sample, &buffer).unwrap();
                assert_eq!(buffer.stream_id, 8);
            }
            assert_eq!(join.join().unwrap().len(), 6);
        }
    }
    #[test]
    fn setup_inspection_gets_mode_settings_and_hssp_without_mutating_commands() {
        let expected = settings();
        let (origin, join) = mock(setup_responses(1, &expected));
        let sample = setup_sample(&broker(&origin)).unwrap();
        assert_eq!(sample.operative_settings, expected);
        assert_eq!(sample.mode.mode_session_id, 42);
        assert_eq!(
            sample.remote_hssp.unwrap().play_state,
            HandyRemotePlayState::Stopped
        );
        assert_eq!(
            join.join().unwrap(),
            vec![
                "GET /connected HTTP/1.1",
                "GET /info HTTP/1.1",
                "GET /mode HTTP/1.1",
                "GET /settings/slider HTTP/1.1",
                "GET /slider/stroke HTTP/1.1",
                "GET /hssp/state HTTP/1.1"
            ]
        );
    }
    #[test]
    fn setup_inspection_skips_hssp_outside_mode_and_withholds_numeric_secret_echo() {
        let (origin, join) = mock(setup_responses(7, &settings()));
        assert!(setup_sample(&broker(&origin))
            .unwrap()
            .remote_hssp
            .is_none());
        assert_eq!(join.join().unwrap().len(), 5);
        let mut echoed = settings();
        echoed.slider.x_limit_stop = 42424242.0;
        let (origin, join) = mock_with_keys(setup_responses(7, &echoed), "fixture-api", "42424242");
        let broker = Broker {
            origin: &origin,
            credentials: Arc::new(Credentials {
                api: "fixture-api".into(),
                connection: "42424242".into(),
            }),
            deadline: Instant::now() + Duration::from_secs(3),
        };
        let error = setup_sample(&broker).unwrap_err();
        assert!(error.message.contains("withheld"));
        assert!(!error.message.contains("42424242"));
        join.join().unwrap();
    }
    #[test]
    fn hsp_clock_read_preserves_stable_context_while_ring_counters_advance() {
        let state = |time: i32| {
            serde_json::json!({"result":{"play_state":1,"points":2,"max_points":100.0,
            "current_point":0,"current_time":time,"loop":false,"playback_rate":1.0,
            "first_point_time":0,"last_point_time":1000,"stream_id":42,"tail_point_stream_index":1,
            "tail_point_stream_index_threshold":0,"pause_on_starving":false}})
            .to_string()
        };
        let mut responses = setup_responses(4, &settings());
        responses.push((200, state(100)));
        for uptime in [100, 101] {
            responses.push((
                200,
                serde_json::json!({"result":{"time":uptime,"clock_offset":-125,"rtd":10}})
                    .to_string(),
            ));
            responses.push((200, r#"{"result":{"offset":-50}}"#.into()));
        }
        responses.extend(setup_responses(4, &settings()));
        responses.push((200, state(102)));
        let (origin, worker) = mock(responses);
        let mut now = 100u64;
        let read = raw_timing_read(&broker(&origin), &mut || {
            now += 1;
            Ok(now)
        })
        .unwrap();
        validate_hsp_clock_context(&read, 42).unwrap();
        assert!(!read.device.before.same_state(&read.device.after));
        let paths = worker.join().unwrap();
        assert_eq!(paths.len(), 16);
        assert_eq!(
            paths
                .iter()
                .filter(|p| p.as_str() == "GET /hsp/state HTTP/1.1")
                .count(),
            2
        );
        assert!(paths.iter().all(|p| p.starts_with("GET ")));
        for case in 0..7 {
            let mut bad = read.clone();
            let after = bad.device.after.remote_hsp.as_mut().unwrap();
            match case {
                0 => after.stream_id += 1,
                1 => after.play_state = HandyRemotePlayState::Stopped,
                2 => after.playback_rate = 2.0,
                3 => after.looping = true,
                4 => after.pause_on_starving = true,
                5 => after.max_points -= 1,
                _ => bad.device.second.user_offset.offset_ms += 1,
            }
            assert!(validate_hsp_clock_context(&bad, 42).is_err(), "case {case}");
        }
    }
    #[test]
    fn hsp_inspection_keeps_signed_state_and_uses_only_the_hsp_get_endpoint() {
        let mut responses = setup_responses(4, &settings());
        responses.push((200, r#"{"result":{"play_state":0,"points":0,"max_points":100.0,"current_point":-1,"current_time":-250,"loop":false,"playback_rate":0.0,"first_point_time":0,"last_point_time":0,"stream_id":0,"tail_point_stream_index":-1,"tail_point_stream_index_threshold":0,"pause_on_starving":false}}"#.into()));
        let (origin, join) = mock(responses);
        let sample = setup_sample(&broker(&origin)).unwrap();
        assert!(sample.remote_hssp.is_none());
        let hsp = sample.remote_hsp.unwrap();
        assert_eq!(hsp.play_state, HandyRemotePlayState::NotInitialized);
        assert_eq!(hsp.current_time_ms, -250);
        assert_eq!(hsp.current_point, -1);
        assert_eq!(hsp.tail_point_stream_index, -1);
        assert_eq!(hsp.playback_rate, 0.0);
        let paths = join.join().unwrap();
        assert_eq!(paths.len(), 6);
        assert_eq!(paths.last().unwrap(), "GET /hsp/state HTTP/1.1");
        assert!(paths.iter().all(|path| path.starts_with("GET ")));
    }

    #[test]
    fn hsp_inspection_withholds_numeric_secret_echo() {
        let mut responses = setup_responses(4, &settings());
        responses.push((200, r#"{"result":{"play_state":2,"points":0,"max_points":100.0,"current_point":-1,"current_time":0,"loop":false,"playback_rate":1.0,"first_point_time":0,"last_point_time":0,"stream_id":42424242,"tail_point_stream_index":-1,"tail_point_stream_index_threshold":0,"pause_on_starving":false}}"#.into()));
        let (origin, join) = mock_with_keys(responses, "fixture-api", "42424242");
        let broker = Broker {
            origin: &origin,
            credentials: Arc::new(Credentials {
                api: "fixture-api".into(),
                connection: "42424242".into(),
            }),
            deadline: Instant::now() + Duration::from_secs(3),
        };
        let error = setup_sample(&broker).unwrap_err();
        assert!(error.message.contains("withheld"));
        assert!(!error.message.contains("42424242"));
        assert_eq!(join.join().unwrap().len(), 6);
    }

    #[test]
    fn handy_qualification_sampling_checks_both_settings_reads_and_refuses_changes() {
        for changed in [false, true] {
            let initial = settings();
            let mut final_settings = initial.clone();
            if changed {
                final_settings.slider.overclock_enabled = true;
            }
            let mut responses = setup_responses(7, &initial);
            responses.extend(setup_responses(7, &final_settings));
            let (origin, join) = mock(responses);
            let result = qualification_sample(&broker(&origin));
            assert_eq!(result.is_ok(), !changed);
            assert_eq!(join.join().unwrap().len(), 10);
        }
    }
}
