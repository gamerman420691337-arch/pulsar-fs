//! Fixed-origin temporary script hosting. No device credential or actuation path.
use pulsar_protocol::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    time::{Duration, Instant},
};
const ORIGIN: &str = "https://www.handyfeeling.com";
const UPLOAD: &str = "/api/hosting/v2/upload";
const RESPONSE_LIMIT: u64 = 64 * 1024;
const BUDGET: Duration = Duration::from_secs(30);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UnknownHostingOutcome;
/// Native-only location. No Debug or Serialize implementation.
#[derive(Clone)]
pub(crate) struct VerifiedHosted {
    location: String,
    source: ArtifactIdentity,
    samples: SampleEnvelope,
    pub(crate) downloaded: ArtifactIdentity,
}
impl VerifiedHosted {
    pub(crate) fn sample_shape(&self) -> HandyScriptSampleShape {
        self.samples.shape.clone()
    }
    /// Private URL remains in a native request body, never a public report or journal.
    pub(crate) fn setup_body(&self) -> Result<Vec<u8>, UnknownHostingOutcome> {
        if location_path(ORIGIN, &self.location).is_none() {
            return Err(UnknownHostingOutcome);
        }
        serde_json::to_vec(&serde_json::json!({"url":self.location,"notify":false}))
            .map_err(|_| UnknownHostingOutcome)
    }
    pub(crate) fn same_location(&self, other: &Self) -> bool {
        self.location == other.location
            && self.source == other.source
            && self.downloaded == other.downloaded
            && self.samples == other.samples
    }
    pub(crate) fn recheck(
        &self,
        deadline: Instant,
    ) -> Result<ArtifactIdentity, UnknownHostingOutcome> {
        recheck_at(ORIGIN, self, deadline)
    }
    pub(crate) fn matches(&self, report: &HandyHostingReport) -> bool {
        report.script == self.source
            && report.downloaded.as_ref() == Some(&self.downloaded)
            && location_path(ORIGIN, &self.location).is_some()
    }
    /// Checks discrete samples only. Native interpolation, physical dynamics,
    /// hardware settings and operative playback timing remain separate admission inputs.
    pub(crate) fn check_profile(
        &self,
        profile: &HandyHsspQualificationProfile,
    ) -> Result<(), ProtocolError> {
        profile.validate()?;
        if self.samples.minimum_position < u32::from(profile.minimum_position_percent)
            || self.samples.maximum_position > u32::from(profile.maximum_position_percent)
        {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "Handy script position lies outside the accepted sample range",
            ));
        }
        if self.samples.minimum_segment_ms < profile.minimum_segment_ms {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "Handy script segment interval is below the accepted sample minimum",
            ));
        }
        if self.samples.maximum_step > u32::from(profile.maximum_step_percent) {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "Handy script step exceeds the accepted sample maximum",
            ));
        }
        Ok(())
    }
}
fn location_path<'a>(origin: &str, location: &'a str) -> Option<&'a str> {
    let path = location.strip_prefix(origin)?;
    let id = [
        "/api/hosting/v2/download/",
        "/api/hosting/v0/download/",
        "/api/script/v0/hosting/download/",
    ]
    .iter()
    .find_map(|prefix| path.strip_prefix(prefix))?;
    (!id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'))
    .then_some(path)
}
fn rows(
    bytes: &[u8],
) -> Result<
    impl Iterator<Item = Result<(u64, u32), UnknownHostingOutcome>> + '_,
    UnknownHostingOutcome,
> {
    let text = std::str::from_utf8(bytes).map_err(|_| UnknownHostingOutcome)?;
    Ok(text.lines().filter_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        Some((|| {
            let (time, pos) = line.split_once(',').ok_or(UnknownHostingOutcome)?;
            let time = time.trim();
            let pos = pos.trim();
            if time.is_empty()
                || pos.is_empty()
                || !time.bytes().all(|b| b.is_ascii_digit())
                || !pos.bytes().all(|b| b.is_ascii_digit())
            {
                return Err(UnknownHostingOutcome);
            }
            let t = time.parse::<u64>().map_err(|_| UnknownHostingOutcome)?;
            let p = pos.parse::<u32>().map_err(|_| UnknownHostingOutcome)?;
            if t > (1u64 << 53) - 1 || p > 100 {
                return Err(UnknownHostingOutcome);
            }
            Ok((t, p))
        })())
    }))
}
#[derive(Clone, PartialEq, Eq)]
struct SampleEnvelope {
    shape: HandyScriptSampleShape,
    minimum_position: u32,
    maximum_position: u32,
    minimum_segment_ms: u64,
    maximum_step: u32,
}
fn verify_rows(
    expected: &[u8],
    downloaded: &[u8],
) -> Result<SampleEnvelope, UnknownHostingOutcome> {
    let mut expected = rows(expected)?;
    let mut downloaded = rows(downloaded)?;
    let mut last: Option<(u64, u32)> = None;
    let mut count = 0u64;
    let mut samples = SampleEnvelope {
        shape: HandyScriptSampleShape {
            points: 0,
            first_time_ms: 0,
            last_time_ms: 0,
        },
        minimum_position: 100,
        maximum_position: 0,
        minimum_segment_ms: u64::MAX,
        maximum_step: 0,
    };
    loop {
        match (expected.next(), downloaded.next()) {
            (None, None) => break,
            (Some(a), Some(b)) => {
                let a = a?;
                let b = b?;
                if a != b || last.is_some_and(|(t, _)| t >= a.0) {
                    return Err(UnknownHostingOutcome);
                }
                if let Some((t, p)) = last {
                    samples.minimum_segment_ms = samples.minimum_segment_ms.min(a.0 - t);
                    samples.maximum_step = samples.maximum_step.max(a.1.abs_diff(p));
                }
                samples.minimum_position = samples.minimum_position.min(a.1);
                samples.maximum_position = samples.maximum_position.max(a.1);
                if count == 0 {
                    samples.shape.first_time_ms = a.0;
                }
                samples.shape.last_time_ms = a.0;
                last = Some(a);
                count += 1;
                if count > HANDY_STROKE_SCRIPT_MAX_ACTIONS {
                    return Err(UnknownHostingOutcome);
                }
            }
            _ => return Err(UnknownHostingOutcome),
        }
    }
    if count < 2 {
        return Err(UnknownHostingOutcome);
    }
    samples.shape.points = count;
    samples
        .shape
        .validate()
        .map_err(|_| UnknownHostingOutcome)?;
    Ok(samples)
}
fn agent(deadline: Instant) -> Result<ureq::Agent, UnknownHostingOutcome> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or(UnknownHostingOutcome)?;
    Ok(ureq::Agent::config_builder()
        .timeout_global(Some(remaining))
        .max_redirects(0)
        .proxy(None)
        .build()
        .into())
}
fn body(
    mut response: ureq::http::Response<ureq::Body>,
    limit: u64,
    deadline: Instant,
) -> Result<Vec<u8>, UnknownHostingOutcome> {
    if response.status().as_u16() != 200 {
        return Err(UnknownHostingOutcome);
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| UnknownHostingOutcome)?;
    if bytes.len() as u64 > limit || Instant::now() >= deadline {
        return Err(UnknownHostingOutcome);
    }
    Ok(bytes)
}
fn recheck_at(
    origin: &str,
    hosted: &VerifiedHosted,
    deadline: Instant,
) -> Result<ArtifactIdentity, UnknownHostingOutcome> {
    let path = location_path(origin, &hosted.location).ok_or(UnknownHostingOutcome)?;
    let response = agent(deadline)?
        .get(format!("{origin}{path}"))
        .header("Accept", "text/csv")
        .call()
        .map_err(|_| UnknownHostingOutcome)?;
    let bytes = body(response, HANDY_HOSTED_SCRIPT_MAX_BYTES, deadline)?;
    if bytes.len() as u64 != hosted.downloaded.byte_len
        || format!("{:x}", Sha256::digest(&bytes)) != hosted.downloaded.sha256
    {
        return Err(UnknownHostingOutcome);
    }
    Ok(hosted.downloaded.clone())
}

fn execute(
    origin: &str,
    bytes: &[u8],
    deadline: Instant,
) -> Result<VerifiedHosted, UnknownHostingOutcome> {
    if bytes.is_empty() || bytes.len() as u64 > HANDY_STROKE_SCRIPT_MAX_BYTES {
        return Err(UnknownHostingOutcome);
    }
    verify_rows(bytes, bytes)?;
    // Input is checked numeric CSV/comment text; an unpredictable delimiter also
    // excludes accidental multipart framing overlap. No local filename is shared.
    let boundary = format!("pulsar{}", uuid::Uuid::new_v4().simple());
    if bytes
        .windows(boundary.len())
        .any(|part| part == boundary.as_bytes())
    {
        return Err(UnknownHostingOutcome);
    }
    let mut payload=format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"script.csv\"\r\nContent-Type: text/csv\r\n\r\n").into_bytes();
    payload.extend_from_slice(bytes);
    payload.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let response = agent(deadline)?
        .post(format!("{origin}{UPLOAD}"))
        .header("Accept", "application/json")
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .send(payload.as_slice())
        .map_err(|_| UnknownHostingOutcome)?;
    let reply = body(response, RESPONSE_LIMIT, deadline)?;
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Uploaded {
        url: String,
    }
    let reply: Uploaded = serde_json::from_slice(&reply).map_err(|_| UnknownHostingOutcome)?;
    let path = location_path(origin, &reply.url).ok_or(UnknownHostingOutcome)?;
    let response = agent(deadline)?
        .get(format!("{origin}{path}"))
        .header("Accept", "text/csv")
        .call()
        .map_err(|_| UnknownHostingOutcome)?;
    let downloaded = body(response, HANDY_HOSTED_SCRIPT_MAX_BYTES, deadline)?;
    let samples = verify_rows(bytes, &downloaded)?;
    let sha256 = format!("{:x}", Sha256::digest(&downloaded));
    let source_sha256 = format!("{:x}", Sha256::digest(bytes));
    Ok(VerifiedHosted {
        location: reply.url,
        source: ArtifactIdentity {
            artifact_id: ArtifactId::new(source_sha256.clone())
                .map_err(|_| UnknownHostingOutcome)?,
            sha256: source_sha256,
            byte_len: bytes.len() as u64,
        },
        samples,
        downloaded: ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).map_err(|_| UnknownHostingOutcome)?,
            sha256,
            byte_len: downloaded.len() as u64,
        },
    })
}
pub(crate) fn host(bytes: &[u8]) -> Result<VerifiedHosted, UnknownHostingOutcome> {
    execute(ORIGIN, bytes, Instant::now() + BUDGET)
}
#[cfg(test)]
pub(crate) fn fixture_verified(bytes: &[u8]) -> VerifiedHosted {
    let sha256 = format!("{:x}", Sha256::digest(bytes));
    VerifiedHosted {
        location: format!("{ORIGIN}/api/hosting/v2/download/fixture"),
        source: ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256: sha256.clone(),
            byte_len: bytes.len() as u64,
        },
        samples: verify_rows(bytes, bytes).expect("valid fixture samples"),
        downloaded: ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: bytes.len() as u64,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        net::{TcpListener, TcpStream},
        thread,
    };
    const CSV: &[u8] = b"# fixture\n0,20\n1000,80\n";
    // Declared limits for authored software fixtures, never physical qualification.
    fn sample_profile() -> HandyHsspQualificationProfile {
        HandyHsspQualificationProfile {
            dynamics: DynamicsProfile {
                axes: vec![AxisDynamics {
                    axis: Axis::Stroke,
                    unit: "mm".into(),
                    minimum: 0.0,
                    maximum: 100.0,
                    max_velocity: 1e6,
                    max_acceleration: 1e6,
                    max_jerk: 1e6,
                }],
                coupled: vec![],
                minimum_gain: 0.0,
            },
            minimum_segment_ms: 1000,
            maximum_step_percent: 60,
            minimum_position_percent: 20,
            maximum_position_percent: 80,
            maximum_command_age_ns: 1_000_000,
            maximum_clock_uncertainty_ns: 1_000_000,
            drift_ppm: 10,
            stopping_bound_ns: None,
        }
    }
    #[test]
    fn handy_hosting_sample_envelope_checks_inclusive_limits_and_every_segment() {
        let hosted = fixture_verified(CSV);
        let profile = sample_profile();
        hosted.check_profile(&profile).unwrap();
        for changed in 0..4 {
            let mut p = profile.clone();
            match changed {
                0 => p.minimum_position_percent = 21,
                1 => p.maximum_position_percent = 79,
                2 => p.minimum_segment_ms = 1001,
                _ => p.maximum_step_percent = 59,
            }
            assert_eq!(
                hosted.check_profile(&p).unwrap_err().code,
                ErrorCode::Forbidden
            );
        }
        for bytes in [
            b"0,20\n1000,81\n2000,20\n".as_slice(),
            b"0,20\n1000,19\n2000,20\n".as_slice(),
            b"0,20\n999,80\n2000,20\n".as_slice(),
        ] {
            assert_eq!(
                fixture_verified(bytes)
                    .check_profile(&profile)
                    .unwrap_err()
                    .code,
                ErrorCode::Forbidden
            );
        }
        // An interior descent must be checked as well as ascent and endpoints.
        let mut p = profile.clone();
        p.maximum_step_percent = 50;
        assert!(fixture_verified(b"0,40\n1000,80\n2000,20\n3000,40\n")
            .check_profile(&p)
            .is_err());
        let mut invalid = profile;
        invalid.minimum_segment_ms = 0;
        assert!(hosted.check_profile(&invalid).is_err());
    }
    #[test]
    fn handy_hosting_sample_envelope_uses_exported_quantization() {
        let program = MotionProgram::new(vec![MotionTrack::new(
            Axis::Stroke,
            vec![
                MotionAction::new(
                    ProjectTime::from_nanos(0),
                    NormalizedPosition::new(0.196).unwrap(),
                    EvidenceKind::Synthesized,
                )
                .unwrap(),
                MotionAction::new(
                    ProjectTime::from_nanos(1_500_000),
                    NormalizedPosition::new(0.804).unwrap(),
                    EvidenceKind::Synthesized,
                )
                .unwrap(),
            ],
        )
        .unwrap()])
        .unwrap();
        let (bytes, _) = crate::handy_script::prepare(&program).unwrap();
        assert_eq!(bytes, b"# Pulsar stroke samples v1\n0,20\n2,80\n");
        let mut profile = sample_profile();
        profile.minimum_segment_ms = 2;
        fixture_verified(&bytes).check_profile(&profile).unwrap();
        profile.minimum_segment_ms = 3;
        assert!(fixture_verified(&bytes).check_profile(&profile).is_err());
    }

    fn accept(listener: &TcpListener) -> TcpStream {
        listener.set_nonblocking(true).unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            match listener.accept() {
                Ok((s, _)) => return s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < end, "fixture accept deadline");
                    thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("fixture accept: {e}"),
            }
        }
    }
    struct Reply {
        status: u16,
        body: String,
        location: Option<String>,
        delay: Duration,
    }
    fn reply(body: impl Into<String>) -> Reply {
        Reply {
            status: 200,
            body: body.into(),
            location: None,
            delay: Duration::ZERO,
        }
    }
    fn server(build: impl FnOnce(&str) -> Vec<Reply>) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let replies = build(&origin);
        let worker = thread::spawn(move || {
            let mut paths = Vec::new();
            for response in replies {
                let mut stream = accept(&listener);
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut headers = String::new();
                loop {
                    let mut line = String::new();
                    assert!(reader.read_line(&mut line).unwrap() > 0);
                    if line == "\r\n" {
                        break;
                    }
                    headers.push_str(&line);
                    assert!(headers.len() < 8192);
                }
                let lower = headers.to_ascii_lowercase();
                assert!(
                    !lower.contains("authorization:")
                        && !lower.contains("x-api-key:")
                        && !lower.contains("x-connection-key:")
                );
                let first = headers.lines().next().unwrap().to_owned();
                if first.starts_with("POST ") {
                    let length: usize = lower
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    assert!(length < 4096);
                    let mut body = vec![0; length];
                    reader.read_exact(&mut body).unwrap();
                    let body = String::from_utf8(body).unwrap();
                    assert!(body.contains("name=\"file\"; filename=\"script.csv\""));
                    assert!(body.contains(std::str::from_utf8(CSV).unwrap()));
                    assert!(!body.contains("project_id") && !body.contains("/home/"));
                }
                paths.push(first);
                thread::sleep(response.delay);
                let location = response
                    .location
                    .map(|s| format!("Location: {s}\r\n"))
                    .unwrap_or_default();
                let _ = write!(
                    stream,
                    "HTTP/1.1 {} Fixture\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n{}",
                    response.status,
                    response.body.len(),
                    location,
                    response.body
                );
            }
            paths
        });
        (origin, worker)
    }
    #[test]
    fn handy_hosting_upload_then_exact_row_verification_has_no_credentials() {
        let (origin, job) = server(|origin| {
            vec![
                reply(format!(
                    "{{\"url\":\"{origin}/api/hosting/v2/download/abc123\"}}"
                )),
                reply("# converted\r\n0000,020\r\n1000,80\r\n"),
            ]
        });
        let result = execute(&origin, CSV, Instant::now() + Duration::from_secs(5)).unwrap();
        result.check_profile(&sample_profile()).unwrap();
        assert_eq!(result.source.sha256, format!("{:x}", Sha256::digest(CSV)));
        assert_ne!(result.source, result.downloaded);
        assert_eq!(
            result.downloaded.sha256,
            format!(
                "{:x}",
                Sha256::digest(b"# converted\r\n0000,020\r\n1000,80\r\n")
            )
        );
        assert_eq!(
            job.join().unwrap(),
            vec![
                "POST /api/hosting/v2/upload HTTP/1.1",
                "GET /api/hosting/v2/download/abc123 HTTP/1.1"
            ]
        );
    }
    #[test]
    fn handy_hosting_rejects_untrusted_urls_and_error_envelopes_without_get() {
        for url in [
            "https://attacker.invalid/api/hosting/v2/download/x",
            "https://www.handyfeeling.com.evil/api/hosting/v2/download/x",
            "https://www.handyfeeling.com/api/hosting/v2/download/../x",
            "https://www.handyfeeling.com/api/hosting/v2/download/x?q=secret",
            "https://www.handyfeeling.com/api/hosting/v2/download/x#fragment",
            "http://127.0.0.1/api/hosting/v2/download/x",
        ] {
            assert!(location_path(ORIGIN, url).is_none());
        }
        for body in ["{\"url\":\"https://attacker.invalid/x\"}".to_string(),"{\"error\":\"withheld\",\"url\":\"https://www.handyfeeling.com/api/hosting/v2/download/x\"}".into(),"x".repeat(RESPONSE_LIMIT as usize+1)] {
            let (origin,job)=server(|_|vec![reply(body)]);assert!(execute(&origin,CSV,Instant::now()+Duration::from_secs(5)).is_err());assert_eq!(job.join().unwrap().len(),1);
        }
    }
    #[test]
    fn handy_hosting_refuses_redirect_failure_and_changed_rows_without_retry() {
        for status in [302, 401, 429, 500] {
            let (origin, job) = server(|o| {
                vec![Reply {
                    status,
                    body: String::new(),
                    location: Some(format!("{o}/elsewhere")),
                    delay: Duration::ZERO,
                }]
            });
            assert!(execute(&origin, CSV, Instant::now() + Duration::from_secs(5)).is_err());
            assert_eq!(job.join().unwrap().len(), 1);
        }
        for csv in [
            "0,20\n1000,81\n",
            "1000,80\n0,20\n",
            "0,20\n0,80\n",
            "0,20\n1000,80\n1001,80\n",
            "0,20\n1000,80.0\n",
        ] {
            let (origin, job) = server(|o| {
                vec![
                    reply(format!("{{\"url\":\"{o}/api/hosting/v0/download/id\"}}")),
                    reply(csv),
                ]
            });
            assert!(execute(&origin, CSV, Instant::now() + Duration::from_secs(5)).is_err());
            assert_eq!(job.join().unwrap().len(), 2);
        }
    }
    #[test]
    fn handy_hosting_absolute_deadline_covers_upload_and_download() {
        let (origin, job) = server(|o| {
            vec![
                Reply {
                    status: 200,
                    body: format!("{{\"url\":\"{o}/api/hosting/v2/download/id\"}}"),
                    location: None,
                    delay: Duration::from_millis(600),
                },
                Reply {
                    status: 200,
                    body: String::from_utf8(CSV.to_vec()).unwrap(),
                    location: None,
                    delay: Duration::from_millis(600),
                },
            ]
        });
        assert!(execute(&origin, CSV, Instant::now() + Duration::from_secs(1)).is_err());
        assert_eq!(job.join().unwrap().len(), 2);
    }
    #[test]
    fn setup_recheck_uses_only_get_and_requires_original_download_bytes() {
        let (origin, job) = server(|_| vec![reply(String::from_utf8(CSV.to_vec()).unwrap())]);
        let mut hosted = fixture_verified(CSV);
        hosted.location = format!("{origin}/api/hosting/v2/download/original");
        assert_eq!(
            recheck_at(&origin, &hosted, Instant::now() + Duration::from_secs(5)).unwrap(),
            hosted.downloaded
        );
        assert_eq!(
            job.join().unwrap(),
            vec!["GET /api/hosting/v2/download/original HTTP/1.1"]
        );
        let changed = "# altered\n0,20\n1000,80\n";
        assert_eq!(changed.len(), CSV.len());
        let (origin, job) = server(|_| vec![reply(changed)]);
        hosted.location = format!("{origin}/api/hosting/v2/download/original");
        assert!(recheck_at(&origin, &hosted, Instant::now() + Duration::from_secs(5)).is_err());
        job.join().unwrap();
    }
}
