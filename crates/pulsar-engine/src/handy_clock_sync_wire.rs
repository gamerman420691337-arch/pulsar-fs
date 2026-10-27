//! One effectful GET. No redirects, retries, offset writes or playback operations.
use crate::{
    device_effect_journal::ClockSyncCompletion, handy_credentials::Credentials, handy_wire,
};
use pulsar_protocol::{HandyClockSyncAcknowledgment, ProtocolError};
type PResult<T> = std::result::Result<T, ProtocolError>;
use std::{io::Read, sync::Arc, time::Instant};
const ORIGIN: &str = "https://www.handyfeeling.com/api/handy-rest/v3";
pub(crate) fn send(
    credentials: Arc<Credentials>,
    deadline: Instant,
    clock: &mut dyn FnMut() -> PResult<u64>,
) -> ClockSyncCompletion {
    execute(ORIGIN, credentials, deadline, clock)
        .map(ClockSyncCompletion::Acknowledged)
        .unwrap_or(ClockSyncCompletion::Unknown)
}
fn execute(
    origin: &str,
    credentials: Arc<Credentials>,
    deadline: Instant,
    clock: &mut dyn FnMut() -> PResult<u64>,
) -> Option<HandyClockSyncAcknowledgment> {
    credentials.validate().ok()?;
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
    let mut request = agent
        .get(format!("{origin}/hstp/clocksync?s=true"))
        .header("X-Api-Key", &credentials.api)
        .header("X-Connection-Key", &credentials.connection)
        .header("Accept", "application/json")
        .header("Cache-Control", "no-cache, no-store");
    for name in ["X-Api-Key", "X-Connection-Key"] {
        request.headers_mut()?.get_mut(name)?.set_sensitive(true);
    }
    let sent_boottime_ns = clock().ok()?;
    let mut response = request.call().ok()?;
    let status = response.status().as_u16();
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(handy_wire::RESPONSE_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    let received_boottime_ns = clock().ok()?;
    if Instant::now() >= deadline {
        return None;
    }
    let a = HandyClockSyncAcknowledgment {
        info: handy_wire::decode_device_time(status, &bytes).ok()?,
        sent_boottime_ns,
        received_boottime_ns,
    };
    a.validate().ok()?;
    if credentials.echoed_text(&serde_json::to_string(&a).ok()?) {
        return None;
    }
    Some(a)
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
    fn run(
        response: Option<String>,
        delay: Duration,
        key: &str,
    ) -> (Option<HandyClockSyncAcknowledgment>, usize) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let key = key.to_owned();
        let keys = Arc::new(Credentials {
            api: "fixture-api".into(),
            connection: key.clone(),
        });
        let worker = thread::spawn(move || {
            let until = Instant::now() + Duration::from_secs(3);
            let mut count = 0;
            let mut drain = None;
            loop {
                if drain.is_some_and(|d| Instant::now() >= d) {
                    break;
                }
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        count += 1;
                        socket
                            .set_read_timeout(Some(Duration::from_secs(1)))
                            .unwrap();
                        socket
                            .set_write_timeout(Some(Duration::from_secs(1)))
                            .unwrap();
                        let mut reader = BufReader::new(socket.try_clone().unwrap());
                        let mut line = String::new();
                        reader.read_line(&mut line).unwrap();
                        assert_eq!(line.trim(), "GET /hstp/clocksync?s=true HTTP/1.1");
                        let mut headers = String::new();
                        loop {
                            line.clear();
                            reader.read_line(&mut line).unwrap();
                            if line == "\r\n" {
                                break;
                            }
                            headers.push_str(&line);
                        }
                        assert!(headers.to_lowercase().contains("x-api-key: fixture-api"));
                        assert!(headers
                            .to_lowercase()
                            .contains(&format!("x-connection-key: {key}")));
                        thread::sleep(delay);
                        if let Some(text) = &response {
                            let _ = socket.write_all(text.as_bytes());
                        }
                        drop(socket);
                        drain = Some(Instant::now() + Duration::from_millis(100));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < until);
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(e) => panic!("fixture accept: {e}"),
                }
            }
            count
        });
        let mut tick = 100;
        let result = execute(
            &origin,
            keys,
            Instant::now() + Duration::from_millis(80),
            &mut || {
                tick += 1;
                Ok(tick)
            },
        );
        (result, worker.join().unwrap())
    }
    #[test]
    fn handy_clock_sync_wire_one_effectful_get_and_no_retry_on_lost_truncated_error_or_redirect_reply(
    ) {
        let body = r#"{"result":{"time":100,"clock_offset":125,"rtd":10}}"#;
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let (a, count) = run(Some(reply.clone()), Duration::ZERO, "fixture-device");
        assert_eq!(count, 1);
        assert_eq!(a.unwrap().info.device_uptime_ms, 100);
        for raw in [
            None,
            Some("HTTP/1.1 200".into()),
            Some("HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{}".into()),
            Some("HTTP/1.1 503 Busy\r\nRetry-After: 0\r\nContent-Length: 0\r\n\r\n".into()),
            Some(
                "HTTP/1.1 302 Found\r\nLocation: /must-not-follow\r\nContent-Length: 0\r\n\r\n"
                    .into(),
            ),
        ] {
            let (a, n) = run(raw, Duration::ZERO, "fixture-device");
            assert!(a.is_none());
            assert_eq!(n, 1);
        }
        let (a, n) = run(
            Some(reply.clone()),
            Duration::from_millis(150),
            "fixture-device",
        );
        assert!(a.is_none());
        assert_eq!(n, 1);
        let (a, n) = run(Some(reply), Duration::ZERO, "125");
        assert!(a.is_none());
        assert_eq!(n, 1);
    }
    #[test]
    fn handy_clock_sync_wire_expired_budget_sends_nothing() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let keys = Arc::new(Credentials {
            api: "fixture-api".into(),
            connection: "fixture-device".into(),
        });
        assert!(execute(
            &format!("http://{}", listener.local_addr().unwrap()),
            keys,
            Instant::now() - Duration::from_millis(1),
            &mut || panic!("no clock read")
        )
        .is_none());
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
}
