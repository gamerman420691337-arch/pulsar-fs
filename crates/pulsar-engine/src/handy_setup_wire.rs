//! One bounded setup PUT at the fixed vendor origin. No retry, mode change or play.
use crate::{
    device_effect_journal::SetupCompletion, handy_credentials::Credentials,
    handy_hosting_wire::VerifiedHosted, handy_wire,
};
use std::{io::Read, sync::Arc, time::Instant};
const ORIGIN: &str = "https://www.handyfeeling.com/api/handy-rest/v3";
pub(crate) fn send(
    hosted: &VerifiedHosted,
    credentials: Arc<Credentials>,
    previous_stream: u64,
    deadline: Instant,
) -> SetupCompletion {
    execute(ORIGIN, hosted, credentials, previous_stream, deadline)
        .map(SetupCompletion::Acknowledged)
        .unwrap_or(SetupCompletion::Unknown)
}
fn execute(
    origin: &str,
    hosted: &VerifiedHosted,
    credentials: Arc<Credentials>,
    previous_stream: u64,
    deadline: Instant,
) -> Option<pulsar_protocol::HandySetupAcknowledgment> {
    credentials.validate().ok()?;
    let payload = hosted.setup_body().ok()?;
    let remaining = deadline.checked_duration_since(Instant::now())?;
    if remaining.is_zero() {
        return None;
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(remaining))
        .max_redirects(0)
        .proxy(None)
        .build()
        .into();
    let mut builder = agent
        .put(format!("{origin}/hssp/setup"))
        .header("X-Api-Key", &credentials.api)
        .header("X-Connection-Key", &credentials.connection)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json");
    for name in ["X-Api-Key", "X-Connection-Key"] {
        builder.headers_mut()?.get_mut(name)?.set_sensitive(true);
    }
    let mut response = builder.send(payload.as_slice()).ok()?;
    let status = response.status().as_u16();
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(handy_wire::RESPONSE_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if Instant::now() >= deadline {
        return None;
    }
    let observed = handy_wire::decode_setup(status, &bytes).ok()?;
    if observed.stream_id == previous_stream
        || credentials.echoed_text(&serde_json::to_string(&observed).ok()?)
    {
        return None;
    }
    Some(observed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        thread,
        time::Duration,
    };
    fn response() -> serde_json::Value {
        serde_json::json!({"result":{"play_state":2,"pause_on_starving":false,"points":2,"max_points":2.0,"current_point":0,"current_time":0,"loop":false,"playback_rate":1.0,"first_point_time":0,"last_point_time":1000,"stream_id":8,"tail_point_stream_index":1,"tail_point_stream_index_threshold":1}})
    }
    fn run(
        status: u16,
        body: Option<String>,
        delay: Duration,
        connection: &str,
    ) -> Option<pulsar_protocol::HandySetupAcknowledgment> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let key = connection.to_owned();
        let worker = thread::spawn(move || {
            let until = Instant::now() + Duration::from_secs(3);
            let mut socket = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < until);
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut reader = BufReader::new(socket.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert_eq!(line, "PUT /hssp/setup HTTP/1.1\r\n");
            let mut headers = String::new();
            loop {
                line.clear();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                headers.push_str(&line);
            }
            let lower = headers.to_ascii_lowercase();
            assert!(lower.contains("x-api-key: fixture-api"));
            assert!(lower.contains(&format!("x-connection-key: {key}")));
            let length: usize = lower
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            assert!(length < 1024);
            let mut payload = vec![0; length];
            reader.read_exact(&mut payload).unwrap();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&payload).unwrap(),
                serde_json::json!({"notify":false,"url":"https://www.handyfeeling.com/api/hosting/v2/download/fixture"})
            );
            thread::sleep(delay);
            if let Some(body) = body {
                let _=write!(socket,"HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nLocation: https://invalid.example/withheld\r\nConnection: close\r\n\r\n{body}",body.len());
            }
            drop(reader);
            drop(socket);
            let end = Instant::now() + Duration::from_millis(120);
            while Instant::now() < end {
                assert!(
                    matches!(listener.accept(),Err(e) if e.kind()==std::io::ErrorKind::WouldBlock),
                    "setup must not retry or follow a redirect"
                );
                thread::sleep(Duration::from_millis(5));
            }
        });
        let credentials = Arc::new(Credentials {
            api: "fixture-api".into(),
            connection: connection.into(),
        });
        let hosted = crate::handy_hosting_wire::fixture_verified(b"0,20\n1000,20\n");
        let budget = if delay.is_zero() {
            Duration::from_secs(2)
        } else {
            Duration::from_millis(50)
        };
        let result = execute(&origin, &hosted, credentials, 7, Instant::now() + budget);
        worker.join().unwrap();
        result
    }
    #[test]
    fn handy_setup_wire_one_put_requires_fresh_bounded_nonplaying_response() {
        assert_eq!(
            run(
                200,
                Some(response().to_string()),
                Duration::ZERO,
                "fixture-device"
            ),
            Some(handy_wire::fixture_setup_acknowledgment())
        );
        for (field, value) in [
            ("stream_id", serde_json::json!(7)),
            ("play_state", serde_json::json!(1)),
            ("loop", serde_json::json!(true)),
            ("max_points", serde_json::json!(2.5)),
            ("current_time", serde_json::json!(9007199254740992u64)),
        ] {
            let mut r = response();
            r["result"][field] = value;
            assert!(run(200, Some(r.to_string()), Duration::ZERO, "fixture-device").is_none());
        }
    }
    #[test]
    fn handy_setup_wire_errors_dropped_replies_deadlines_and_numeric_echo_stay_unknown_without_retry(
    ) {
        for status in [302, 401, 429, 500] {
            assert!(run(
                status,
                Some(response().to_string()),
                Duration::ZERO,
                "fixture-device"
            )
            .is_none());
        }
        assert!(run(200, None, Duration::ZERO, "fixture-device").is_none());
        assert!(run(
            200,
            Some(response().to_string()),
            Duration::from_millis(150),
            "fixture-device"
        )
        .is_none());
        let mut r = response();
        r["error"] = serde_json::Value::Null;
        assert!(run(200, Some(r.to_string()), Duration::ZERO, "fixture-device").is_none());
        let mut r = response();
        r["result"]["stream_id"] = serde_json::json!(42424242);
        assert!(run(200, Some(r.to_string()), Duration::ZERO, "42424242").is_none());
        assert!(run(
            200,
            Some("x".repeat(handy_wire::RESPONSE_LIMIT + 1)),
            Duration::ZERO,
            "fixture-device"
        )
        .is_none());
    }
}
