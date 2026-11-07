//! The actual HSP implementation against a loopback Handy, never real credentials/hardware.
#[path = "../crates/pulsar-engine/src/desktop_hsp.rs"]
mod hsp;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[test]
fn buffered_hsp_uses_token_auth_shared_clock_and_bounded_uploads() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let events = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
    let recorded = events.clone();
    let server = std::thread::spawn(move || {
        let mut points = 0u64;
        while !stopped.load(Ordering::SeqCst) {
            let (mut socket, _) = match listener.accept() {
                Ok(v) => v,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                }
                Err(e) => panic!("{e}"),
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut buf = [0; 4096];
            let boundary;
            loop {
                let n = socket.read(&mut buf).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buf[..n]);
                assert!(bytes.len() < 65536);
                if let Some(pos) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    boundary = pos + 4;
                    break;
                }
            }
            let header = String::from_utf8(bytes[..boundary].to_vec())
                .unwrap()
                .to_lowercase();
            let route = header
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap()
                .to_string();
            let length = header
                .lines()
                .find_map(|s| s.strip_prefix("content-length:"))
                .map(|s| s.trim().parse::<usize>().unwrap())
                .unwrap_or(0);
            while bytes.len() < boundary + length {
                let n = socket.read(&mut buf).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buf[..n]);
            }
            let body = if length > 0 {
                serde_json::from_slice(&bytes[boundary..boundary + length]).unwrap()
            } else {
                Value::Null
            };
            let response = if route.starts_with("/auth/token/issue?") {
                assert!(header.contains("x-api-key: test-app"));
                json!({"result":{"token":"mock-client-token"}})
            } else {
                assert!(header.contains("authorization: bearer mock-client-token"));
                assert!(header.contains("x-connection-key: test-connection"));
                assert!(!header.contains("x-api-key:"));
                match route.as_str() {
                    "/connected" => json!({"result":{"connected":true}}),
                    "/mode" => json!({"result":{"mode":4}}),
                    "/hsp/setup" => {
                        points = 0;
                        json!({"result":{}})
                    }
                    "/hsp/add" => {
                        points += body["points"].as_array().unwrap().len() as u64;
                        json!({"result":{}})
                    }
                    "/hsp/state" => json!({"result":{"points":points}}),
                    "/servertime" => {
                        json!({"server_time":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64})
                    }
                    "/hstp/offset" => json!({"result":{"offset":-209}}),
                    "/hsp/play" | "/hsp/stop" => json!({"result":{}}),
                    _ => panic!("unexpected route {route}"),
                }
            };
            recorded.lock().unwrap().push((route, body));
            let data = response.to_string();
            write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",data.len(),data).unwrap();
        }
    });
    let mut client = hsp::HspClient::test_client(&base);
    assert!(client.connected().unwrap());
    client.ensure_hsp_mode().unwrap();
    assert_eq!(client.user_offset_ms().unwrap(), -209);
    let points = Arc::new(
        (0..=10)
            .map(|n| (n * 1000, if n % 2 == 0 { 40 } else { 60 }))
            .collect(),
    );
    let mut stream = hsp::HspStream::begin(&mut client, points, 0).unwrap();
    stream.load_until(&mut client, 5000).unwrap();
    assert!(stream.loaded_until(5000));
    assert_eq!(stream.sent(), 6);
    assert_eq!(client.state().unwrap()["points"], 6);
    let clock = client.server_clock(3).unwrap();
    let before = clock.now_ms();
    client.play(0, 1.0, &clock.advanced(1500)).unwrap();
    client.stop().unwrap();
    stop.store(true, Ordering::SeqCst);
    server.join().unwrap();
    let seen = events.lock().unwrap();
    assert_eq!(
        seen.iter()
            .filter(|(r, _)| r.starts_with("/auth/token/issue?"))
            .count(),
        1
    );
    let add = &seen.iter().find(|(r, _)| r == "/hsp/add").unwrap().1;
    assert_eq!(add["tail_point_stream_index"], 5);
    assert_eq!(add["points"].as_array().unwrap().last().unwrap()["t"], 5000);
    let play = &seen.iter().find(|(r, _)| r == "/hsp/play").unwrap().1;
    assert!(play["server_time"].as_u64().unwrap() >= before + 1500);
    assert_eq!(play["loop"], false);
    assert_eq!(seen.last().unwrap().0, "/hsp/stop");
}
