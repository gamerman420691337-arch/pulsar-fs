use super::*;
fn qualified(error: u64, age: u64, budget: u64, drift_ppm: u32) -> QualifiedServerClock {
    QualifiedServerClock {
        contract: HandyServerClockQualification {
            model: HandyServerClockModel::RequestBracketMonotonic,
            timestamp_error_ns: error,
        },
        max_age_ns: age,
        max_uncertainty_ns: budget,
        drift_ppm,
    }
}
fn mapping(rtt: u64, q: QualifiedServerClock) -> Result<NativeServerClock> {
    NativeServerClock::new(
        ArtifactId::new("a".repeat(64)).unwrap(),
        Sample {
            sent_ns: 100,
            received_ns: 100 + rtt,
            server_ms: 1000,
        },
        q,
    )
}
#[test]
fn handy_server_clock_covers_odd_submillisecond_and_asymmetric_delay_intervals() {
    for (rtt, error, ppm, estimate, uncertainty) in [
        (0, 1_000_000, 0, 1000, 1_000_000),
        (3, 0, 0, 1000, 3),
        (999_999, 0, 0, 1000, 999_999),
        (1_000_001, 0, 0, 1001, 1_000_000),
        (1_999_999, 0, 0, 1001, 1_000_000),
        (3, 1_000_000, 0, 1000, 1_000_003),
        (1, 0, 1, 1000, 2),
    ] {
        let native = mapping(rtt, qualified(error, 10_000_000, 10_000_000, ppm)).unwrap();
        assert_eq!(native.clock.estimated_server_ms, estimate);
        assert_eq!(native.clock.uncertainty_ns, uncertainty);
        for age in [0, 1, 999_999, 1_000_000, 1_000_001] {
            let now = native.at(native.clock.monotonic_ns + age).unwrap();
            let center = i128::from(now.estimated_server_ms) * NS_MS as i128;
            let radius = i128::from(now.uncertainty_ns);
            let lower =
                1_000_000_000i128 - i128::from(error) + i128::from(age) - drift(age, ppm) as i128;
            let upper = 1_000_000_000i128
                + i128::from(error)
                + i128::from(rtt + age)
                + drift(rtt, ppm) as i128
                + drift(age, ppm) as i128;
            assert!(
                center - radius <= lower && center + radius >= upper,
                "rtt={rtt} age={age}"
            );
        }
    }
}
#[test]
fn handy_server_clock_refuses_backwards_stale_overflow_and_excess_uncertainty() {
    let n = mapping(10, qualified(1, 20, 1_000_000, 1)).unwrap();
    assert!(n.at(109).is_err());
    assert!(n.at(120).is_ok());
    assert!(n.at(121).is_err());
    assert!(mapping(21, qualified(1, 20, 1_000_000, 0)).is_err());
    assert!(mapping(3, qualified(1_000_000, 20, 1_000_002, 0)).is_err());
    for (sent, received, server, error, ppm) in [
        (2, 1, 1000, 0, 0),
        (0, 0, MAX_MS + 1, 0, 0),
        (0, 0, 0, 1, 0),
        (0, u64::MAX, MAX_MS, u64::MAX, u32::MAX),
    ] {
        assert!(NativeServerClock::new(
            ArtifactId::new("a").unwrap(),
            Sample {
                sent_ns: sent,
                received_ns: received,
                server_ms: server,
            },
            qualified(error, u64::MAX, u64::MAX, ppm)
        )
        .is_err());
    }
    let n = NativeServerClock::new(
        ArtifactId::new("max").unwrap(),
        Sample {
            sent_ns: 0,
            received_ns: 0,
            server_ms: MAX_MS,
        },
        qualified(0, u64::MAX, u64::MAX, 0),
    )
    .unwrap();
    assert!(n.at(1_000_000).is_err());
}
#[test]
fn handy_server_clock_disjoint_valid_samples_refuse_without_averaging() {
    let first = mapping(3, qualified(1, 10_000_000, 2_000_000, 0)).unwrap();
    let mut next = first.clone();
    assert!(next.agrees_with(&first).is_ok());
    next.clock.estimated_server_ms += 10;
    assert!(next.agrees_with(&first).is_err());
}
#[test]
fn handy_server_clock_decoder_rejects_ambiguous_and_error_responses() {
    assert_eq!(decode(br#"{"server_time":123}"#).unwrap(), 123);
    for bad in [
        r#"{"server_time":1,"server_time":2}"#,
        r#"{"server_time":1.0}"#,
        r#"{"server_time":-1}"#,
        r#"{"server_time":"1"}"#,
        r#"{"result":{"server_time":1}}"#,
        r#"{"server_time":1,"error":{}}"#,
        r#"{"server_time":9007199254740992}"#,
        "null",
        "{}",
    ] {
        assert!(decode(bad.as_bytes()).is_err(), "{bad}");
    }
}
#[test]
fn handy_server_clock_http_is_credential_free_bounded_and_does_not_follow_redirects() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };
    for (status, body, delay, expected) in [
        (200, "{\"server_time\":123}".to_owned(), 0, true),
        (302, String::new(), 0, false),
        (500, "{\"server_time\":123}".to_owned(), 0, false),
        (200, " ".repeat(1025), 0, false),
        (200, "{\"server_time\":123}".to_owned(), 180, false),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(v) => break v,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(1))
                    }
                    Err(e) => panic!("fixture accept: {e}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") && request.len() < 8192 {
                let mut b = [0];
                stream.read_exact(&mut b).unwrap();
                request.push(b[0]);
            }
            let request = String::from_utf8(request).unwrap().to_lowercase();
            assert!(request.starts_with("get /servertime http/1.1\r\n"));
            for secret in [
                "x-api-key:",
                "x-connection-key:",
                "authorization:",
                "cookie:",
            ] {
                assert!(!request.contains(secret));
            }
            thread::sleep(Duration::from_millis(delay));
            let _ = write!(stream, "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nLocation: /must-not-follow\r\nConnection: close\r\n\r\n{body}", body.len());
        });
        let mut tick = 100;
        let result = sample_at(
            &origin,
            &mut || {
                tick += 1;
                Ok(tick)
            },
            Duration::from_millis(100),
        );
        assert_eq!(result.is_ok(), expected);
        if let Ok(s) = result {
            assert_eq!((s.sent_ns, s.received_ns, s.server_ms), (101, 102, 123));
        }
        server.join().unwrap();
    }
}
