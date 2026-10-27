//! Engine-owned fixed GraphQL discovery. No raw query, redirects, media upload,
//! downloads, tagging, or scan effects are accepted by this operation.
use pulsar_protocol::*;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    io::Read,
    time::{Duration, Instant},
};

const RESPONSE_LIMIT: u64 = 1024 * 1024;
const DISCOVERY_LIMIT: usize = 512 * 1024;
const QUERY:&str="query PulsarMissing($filter: FindFilterType!) { findScenes(filter: $filter, scene_filter: {interactive: false}) { count scenes { id title interactive files { id path } } } }";
fn unavailable(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::Unavailable, message)
}

fn endpoint(connection: &StashConnection) -> Result<url::Url, ProtocolError> {
    connection.validate()?;
    let parsed = url::Url::parse(&connection.endpoint)
        .map_err(|_| ProtocolError::invalid("invalid Stash endpoint URL"))?;
    let loopback = match parsed.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain(name)) => name == "localhost",
        None => false,
    };
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/graphql"
        || !matches!(parsed.scheme(), "http" | "https")
        || (parsed.scheme() == "http" && !loopback)
    {
        return Err(ProtocolError::invalid("Stash requires /graphql, no URL credentials/query/fragment, and HTTPS outside loopback"));
    }
    Ok(parsed)
}
pub(crate) fn endpoint_identity(connection: &StashConnection) -> Result<String, ProtocolError> {
    Ok(endpoint(connection)?.to_string())
}

#[derive(Deserialize)]
struct Page {
    count: u64,
    scenes: Vec<RawScene>,
}
#[derive(Deserialize)]
struct RawScene {
    id: String,
    title: Option<String>,
    interactive: bool,
    files: Vec<StashVideoFile>,
}

struct Broker {
    url: url::Url,
    key: Option<String>,
    deadline: Instant,
}
impl Broker {
    fn new(connection: &StashConnection) -> Result<Self, ProtocolError> {
        let url = endpoint(connection)?;
        let key=connection.credential_env.as_ref().map(|name|std::env::var(name)
        .map_err(|_|unavailable("Stash credential is absent from the engine environment; launch the engine through secrets run"))).transpose()?;
        if key.as_ref().is_some_and(|key| {
            key.is_empty() || key.len() > 4096 || key.chars().any(char::is_control)
        }) {
            return Err(unavailable(
                "invalid Stash credential in engine environment",
            ));
        }
        // No environment proxy may receive the credential, and no redirect may
        // move it to another origin. One global deadline covers the whole discovery.
        let deadline = Instant::now() + Duration::from_secs(15);
        Ok(Self { url, key, deadline })
    }
    fn call(&self, query: &str, variables: Value) -> Result<Value, ProtocolError> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(unavailable("Stash operation deadline expired"));
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(remaining))
            .max_redirects(0)
            .proxy(None)
            .build()
            .into();
        let body = serde_json::to_vec(&json!({"query":query,"variables":variables}))
            .map_err(|_| unavailable("Stash request serialization failed"))?;
        let mut request = agent
            .post(self.url.as_str())
            .header("Content-Type", "application/json");
        if let Some(key) = &self.key {
            request = request.header("ApiKey", key);
        }
        let mut response = request.send(body.as_slice()).map_err(|_| {
            unavailable("Stash request failed (endpoint, TLS, HTTP status, redirect or timeout)")
        })?;
        if response.status().as_u16() != 200 {
            return Err(unavailable("Stash returned a non-success HTTP status"));
        }
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(RESPONSE_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| unavailable("Stash response read failed"))?;
        if bytes.len() as u64 > RESPONSE_LIMIT {
            return Err(unavailable("Stash response exceeds 1 MiB"));
        }
        let mut value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| unavailable("Stash returned malformed JSON"))?;
        if let Some(errors) = value.get("errors") {
            if !errors.as_array().is_some_and(Vec::is_empty) {
                return Err(unavailable(
                    "Stash GraphQL operation failed; server detail is withheld",
                ));
            }
        }
        let data = value["data"].take();
        if !data.is_object() {
            return Err(unavailable("Stash returned missing data"));
        }
        Ok(data)
    }
    fn checked_scene(&self, raw: RawScene) -> Result<StashScene, ProtocolError> {
        let title = raw.title.unwrap_or_default();
        let text_valid = |text: &str, max: usize| {
            text.len() <= max
                && !text.chars().any(char::is_control)
                && !self.key.as_ref().is_some_and(|key| text.contains(key))
        };
        if raw.id.is_empty()
            || !text_valid(&raw.id, 128)
            || !text_valid(&title, 2048)
            || raw.files.len() > 16
            || raw.files.iter().any(|file| {
                file.id.is_empty() || !text_valid(&file.id, 128) || !text_valid(&file.path, 8192)
            })
        {
            return Err(unavailable(
                "Stash returned oversized, unsafe or credential-bearing scene fields",
            ));
        }
        Ok(StashScene {
            id: raw.id,
            title,
            interactive: raw.interactive,
            files: raw.files,
        })
    }
}

pub(crate) fn scene(connection: &StashConnection, id: &str) -> Result<StashScene, ProtocolError> {
    if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
        return Err(ProtocolError::invalid("invalid Stash scene ID"));
    }
    let client = Broker::new(connection)?;
    let mut data=client.call("query PulsarScene($id: ID!) { findScene(id: $id) { id title interactive files { id path } } }",json!({"id":id}))?;
    let raw: RawScene = serde_json::from_value(data["findScene"].take())
        .map_err(|_| unavailable("Stash scene absent or invalid"))?;
    let scene = client.checked_scene(raw)?;
    if scene.id != id {
        return Err(unavailable("Stash scene identity mismatch"));
    }
    Ok(scene)
}

pub(crate) fn discover(
    connection: &StashConnection,
    limit: u32,
) -> Result<StashDiscovery, ProtocolError> {
    if limit == 0 || limit > 100 {
        return Err(ProtocolError::invalid("Stash limit must be 1..=100"));
    }
    let client = Broker::new(connection)?;
    let mut scenes = Vec::new();
    let mut seen = BTreeSet::new();
    let mut reported_count = 0;
    let mut complete = false;
    let mut total = 0;
    for page in 1..=4_u32 {
        let mut value = client.call(
            QUERY,
            json!({"filter":{"page":page,"per_page":25,"sort":"id","direction":"ASC"}}),
        )?;
        let page: Page = serde_json::from_value(value["findScenes"].take())
            .map_err(|_| unavailable("Stash returned missing or invalid scene data"))?;
        reported_count = page.count;
        if page.scenes.len() > 25 {
            return Err(unavailable("Stash exceeded the declared page size"));
        }
        let last = page.scenes.len() < 25;
        for raw in page.scenes {
            let scene = client.checked_scene(raw)?;
            if scene.interactive || !seen.insert(scene.id.clone()) {
                continue;
            }
            let len = serde_json::to_vec(&scene)
                .map_err(|_| unavailable("Stash scene encoding failed"))?
                .len();
            if total + len > DISCOVERY_LIMIT || scenes.len() >= limit as usize {
                return Ok(StashDiscovery {
                    scenes,
                    reported_count,
                    pagination_complete: false,
                });
            }
            total += len;
            scenes.push(scene);
        }
        if last {
            complete = true;
            break;
        }
        if scenes.len() >= limit as usize {
            break;
        }
    }
    Ok(StashDiscovery {
        scenes,
        reported_count,
        pagination_complete: complete,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::{io::Write, net::TcpListener, thread};
    pub(crate) fn mock(
        responses: Vec<(u16, String, String)>,
    ) -> (StashConnection, thread::JoinHandle<Vec<Value>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let connection = StashConnection {
            endpoint: format!("http://{}/graphql", listener.local_addr().unwrap()),
            credential_env: None,
        };
        let join = thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, headers, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut header_end = None;
                let mut length = 0;
                loop {
                    let mut buffer = [0u8; 4096];
                    let size = stream.read(&mut buffer).unwrap();
                    assert!(size > 0);
                    bytes.extend_from_slice(&buffer[..size]);
                    if header_end.is_none() {
                        if let Some(end) = bytes.windows(4).position(|p| p == b"\r\n\r\n") {
                            let text = String::from_utf8_lossy(&bytes[..end]);
                            length = text
                                .lines()
                                .find_map(|line| {
                                    line.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .and_then(|value| value.trim().parse::<usize>().ok())
                                })
                                .unwrap();
                            header_end = Some(end + 4);
                        }
                    }
                    if header_end.is_some_and(|end| bytes.len() >= end + length) {
                        break;
                    }
                }
                let end = header_end.unwrap();
                requests.push(serde_json::from_slice(&bytes[end..end + length]).unwrap());
                let header=format!("HTTP/1.1 {status} Result\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n",body.len());
                stream.write_all(header.as_bytes()).unwrap();
                let _ = stream.write_all(body.as_bytes());
            }
            requests
        });
        (connection, join)
    }
    fn scene(id: u32) -> Value {
        json!({"id":id.to_string(),"title":null,"interactive":false,"files":[{"id":format!("f{id}"),"path":format!("/server/video{id}.mp4")}]})
    }
    fn page(scenes: Vec<Value>, count: u64) -> String {
        json!({"errors":[],"data":{"findScenes":{"count":count,"scenes":scenes}}}).to_string()
    }
    #[test]
    fn single_scene_lookup_has_fixed_variables_and_rejects_mismatched_identity() {
        let raw = json!({"id":"selected","title":"fixture","interactive":false,"files":[{"id":"file","path":"/server/video.mp4"}]});
        let (connection, server) = mock(vec![(
            200,
            String::new(),
            json!({"data":{"findScene":raw}}).to_string(),
        )]);
        let selected = super::scene(&connection, "selected").unwrap();
        assert_eq!(selected.files[0].id, "file");
        let requests = server.join().unwrap();
        assert_eq!(requests[0]["variables"], json!({"id":"selected"}));
        assert!(requests[0]["query"]
            .as_str()
            .unwrap()
            .contains("findScene(id: $id)"));
        let (connection, server) = mock(vec![(
            200,
            String::new(),
            json!({"data":{"findScene":raw}}).to_string(),
        )]);
        assert!(super::scene(&connection, "different")
            .unwrap_err()
            .message
            .contains("identity mismatch"));
        server.join().unwrap();
        let (connection, server) = mock(vec![(
            200,
            String::new(),
            json!({"data":{"findScene":null}}).to_string(),
        )]);
        assert!(super::scene(&connection, "selected").is_err());
        server.join().unwrap();
    }
    #[test]
    fn real_http_discovery_uses_boolean_filter_and_bounded_pagination() {
        let (connection, server) = mock(vec![
            (200, String::new(), page((1..=25).map(scene).collect(), 26)),
            (200, String::new(), page(vec![scene(26)], 26)),
        ]);
        let discovery = discover(&connection, 100).unwrap();
        assert_eq!(discovery.scenes.len(), 26);
        assert!(discovery.pagination_complete);
        assert_eq!(discovery.scenes[0].title, "");
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0]["query"]
            .as_str()
            .unwrap()
            .contains("interactive: false"));
        assert!(!requests[0]["query"].as_str().unwrap().contains("paths"));
        assert_eq!(requests[1]["variables"]["filter"]["page"], 2);
        assert_eq!(requests[0]["variables"]["filter"]["per_page"], 25);
    }
    #[test]
    fn discovery_caps_selection_deduplicates_and_exposes_incomplete_pagination() {
        let (connection, server) = mock(vec![(
            200,
            String::new(),
            page((1..=25).map(scene).collect(), 50),
        )]);
        let discovery = discover(&connection, 3).unwrap();
        assert_eq!(discovery.scenes.len(), 3);
        assert!(!discovery.pagination_complete);
        server.join().unwrap();
        let mut raced = scene(2);
        raced["interactive"] = Value::Bool(true);
        let (connection, server) = mock(vec![(
            200,
            String::new(),
            page(vec![scene(1), scene(1), raced], 3),
        )]);
        let discovery = discover(&connection, 100).unwrap();
        assert_eq!(discovery.scenes.len(), 1);
        assert!(discovery.pagination_complete);
        server.join().unwrap();
    }
    #[test]
    fn server_errors_null_data_redirects_and_oversized_bodies_fail_without_server_detail() {
        for (status, headers, body) in [
            (
                200,
                "",
                r#"{"errors":[{"message":"secret-api-key"}],"data":null}"#.into(),
            ),
            (200, "", r#"{"data":null}"#.into()),
            (200, "", "not-json".into()),
            (302, "Location: http://127.0.0.1:1/graphql\r\n", "{}".into()),
            (500, "", "secret-api-key".into()),
            (200, "", " ".repeat(RESPONSE_LIMIT as usize + 1)),
        ] {
            let (connection, server) = mock(vec![(status, headers.into(), body)]);
            let error = discover(&connection, 10).unwrap_err();
            assert!(!error.message.contains("secret-api-key"));
            server.join().unwrap();
        }
    }
    #[test]
    fn invalid_limits_and_absent_engine_credentials_fail_before_network() {
        let mut connection = StashConnection {
            endpoint: "http://127.0.0.1:1/graphql".into(),
            credential_env: None,
        };
        assert_eq!(
            discover(&connection, 0).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        connection.credential_env =
            Some(format!("PULSAR_ABSENT_{}", uuid::Uuid::new_v4().simple()).to_ascii_uppercase());
        assert!(discover(&connection, 10)
            .unwrap_err()
            .message
            .contains("absent"));
    }
    #[test]
    fn endpoint_policy_rejects_secret_urls_and_remote_cleartext() {
        for url in [
            "http://example.com/graphql",
            "https://name:key@example.com/graphql",
            "http://localhost:9999/graphql?apikey=secret",
            "http://localhost:9999/graphql#secret",
            "file:///graphql",
            "http://localhost:9999/other",
        ] {
            assert!(endpoint(&StashConnection {
                endpoint: url.into(),
                credential_env: None
            })
            .is_err());
        }
        for url in [
            "http://localhost:9999/graphql",
            "http://127.0.0.1:9999/graphql",
            "http://[::1]:9999/graphql",
            "https://example.com/graphql",
        ] {
            assert!(endpoint(&StashConnection {
                endpoint: url.into(),
                credential_env: None
            })
            .is_ok());
        }
    }
}

pub(crate) fn submit_scan(
    connection: &StashConnection,
    path: &str,
) -> Result<String, ProtocolError> {
    if !path.starts_with('/')
        || path.len() > 8192
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .skip(1)
            .any(|p| p.is_empty() || matches!(p, "." | ".."))
    {
        return Err(ProtocolError::invalid(
            "scan requires one normalized captured server file path",
        ));
    }
    let client = Broker::new(connection)?;
    let data=client.call("mutation PulsarScan($input: ScanMetadataInput!) { metadataScan(input: $input) }",json!({"input":{"paths":[path],"rescan":true,"scanGenerateCovers":false,"scanGeneratePreviews":false,"scanGenerateImagePreviews":false,"scanGenerateSprites":false,"scanGeneratePhashes":false,"scanGenerateImagePhashes":false,"scanGenerateThumbnails":false,"scanGenerateClipPreviews":false}}))?;
    let id = data["metadataScan"]
        .as_str()
        .ok_or_else(|| unavailable("Stash scan acknowledgement lacks a job ID"))?;
    if !client.safe_id(id) {
        return Err(unavailable(
            "Stash scan acknowledgement has an unsafe job ID",
        ));
    }
    Ok(id.into())
}
impl Broker {
    fn safe_id(&self, id: &str) -> bool {
        !id.is_empty()
            && id.len() <= 128
            && !id.chars().any(char::is_control)
            && !self.key.as_ref().is_some_and(|key| id.contains(key))
    }
}
pub(crate) fn scan_state(
    connection: &StashConnection,
    id: &str,
) -> Result<StashScanState, ProtocolError> {
    let client = Broker::new(connection)?;
    if !client.safe_id(id) {
        return Err(ProtocolError::invalid("invalid Stash scan job ID"));
    }
    let data = client.call(
        "query PulsarScanJob($input: FindJobInput!) { findJob(input: $input) { id status } }",
        json!({"input":{"id":id}}),
    )?;
    let job = data
        .get("findJob")
        .ok_or_else(|| unavailable("Stash job response lacks findJob"))?;
    if job.is_null() {
        return Ok(StashScanState::JobMissing);
    }
    if job["id"].as_str() != Some(id) {
        return Err(unavailable("Stash scan job identity mismatch"));
    }
    match job["status"].as_str() {
        Some("READY") => Ok(StashScanState::Ready),
        Some("RUNNING") => Ok(StashScanState::Running),
        Some("STOPPING") => Ok(StashScanState::Stopping),
        Some("CANCELLED") => Ok(StashScanState::Cancelled),
        Some("FAILED") => Ok(StashScanState::Failed),
        Some("FINISHED") => Ok(StashScanState::FinishedUnattached),
        _ => Err(unavailable("Stash returned an unknown scan job status")),
    }
}

#[cfg(test)]
mod scan_tests {
    use super::*;
    #[test]
    fn scan_submission_requires_job_id_and_exact_nonempty_path() {
        let (connection, server) = super::tests::mock(vec![(
            200,
            String::new(),
            json!({"data":{"metadataScan":"job-1"}}).to_string(),
        )]);
        assert_eq!(
            submit_scan(&connection, "/selected/video.mp4").unwrap(),
            "job-1"
        );
        let requests = server.join().unwrap();
        assert_eq!(
            requests[0]["variables"]["input"]["paths"],
            json!(["/selected/video.mp4"])
        );
        assert_eq!(requests[0]["variables"]["input"]["rescan"], true);
        assert!(submit_scan(&connection, "").is_err());
        assert!(submit_scan(&connection, "/selected/../other").is_err());
    }
    #[test]
    fn scan_job_states_and_null_are_distinct() {
        let states = [
            ("READY", StashScanState::Ready),
            ("RUNNING", StashScanState::Running),
            ("STOPPING", StashScanState::Stopping),
            ("FAILED", StashScanState::Failed),
            ("CANCELLED", StashScanState::Cancelled),
            ("FINISHED", StashScanState::FinishedUnattached),
        ];
        let mut responses = states
            .iter()
            .map(|(status, _)| {
                (
                    200,
                    String::new(),
                    json!({"data":{"findJob":{"id":"job","status":status}}}).to_string(),
                )
            })
            .collect::<Vec<_>>();
        responses.push((
            200,
            String::new(),
            json!({"data":{"findJob":null}}).to_string(),
        ));
        let (connection, server) = super::tests::mock(responses);
        for (_, state) in states {
            assert_eq!(scan_state(&connection, "job").unwrap(), state);
        }
        assert_eq!(
            scan_state(&connection, "job").unwrap(),
            StashScanState::JobMissing
        );
        assert_eq!(server.join().unwrap().len(), 7);
    }
    #[test]
    fn malformed_scan_acknowledgements_and_jobs_never_imply_success() {
        let values = [
            json!({"data":{"metadataScan":true}}),
            json!({"data":{"metadataScan":null}}),
            json!({"data":{"findJob":{"id":"wrong","status":"FINISHED"}}}),
            json!({"data":{"findJob":{"id":"job","status":"UNKNOWN"}}}),
            json!({"data":{}}),
        ];
        let (connection, server) = super::tests::mock(
            values
                .into_iter()
                .map(|v| (200, String::new(), v.to_string()))
                .collect(),
        );
        assert!(submit_scan(&connection, "/selected/video.mp4").is_err());
        assert!(submit_scan(&connection, "/selected/video.mp4").is_err());
        for _ in 0..3 {
            assert!(scan_state(&connection, "job").is_err());
        }
        assert_eq!(server.join().unwrap().len(), 5);
    }
}

/// One shared deadline covers tag lookup, optional creation, ADD and verification.
pub(crate) struct TagBroker(Broker);
pub(crate) struct TaggedScene {
    pub scene: StashScene,
    pub tags: Vec<String>,
}
#[derive(Deserialize)]
struct RawTag {
    id: String,
    name: String,
}
impl TagBroker {
    pub(crate) fn new(connection: &StashConnection) -> Result<Self, ProtocolError> {
        Ok(Self(Broker::new(connection)?))
    }
    fn tag(&self, raw: RawTag, name: &str) -> Result<String, ProtocolError> {
        if !self.0.safe_id(&raw.id)
            || raw.name != name
            || name.is_empty()
            || name.len() > 256
            || name.chars().any(char::is_control)
            || self.0.key.as_ref().is_some_and(|key| name.contains(key))
        {
            return Err(unavailable(
                "Stash tag identity/name is unsafe or differs from exact requested name",
            ));
        }
        Ok(raw.id)
    }
    pub(crate) fn lookup(&self, name: &str) -> Result<Option<String>, ProtocolError> {
        if name.is_empty()
            || name.len() > 256
            || name.trim() != name
            || name.chars().any(char::is_control)
            || self.0.key.as_ref().is_some_and(|key| name.contains(key))
        {
            return Err(ProtocolError::invalid("invalid Stash tag name"));
        }
        let mut data=self.0.call("query PulsarTag($name: String!) { findTags(tag_filter: {name: {value: $name, modifier: EQUALS}}, filter: {page: 1, per_page: 2, sort: \"id\", direction: ASC}) { count tags { id name } } }",json!({"name":name}))?;
        let count = data["findTags"]["count"]
            .as_u64()
            .ok_or_else(|| unavailable("Stash tag lookup lacks count"))?;
        let tags: Vec<RawTag> = serde_json::from_value(data["findTags"]["tags"].take())
            .map_err(|_| unavailable("invalid Stash tag lookup"))?;
        match (count, tags.len()) {
            (0, 0) => Ok(None),
            (1, 1) => self
                .tag(tags.into_iter().next().expect("one tag"), name)
                .map(Some),
            _ => Err(unavailable(
                "Stash exact tag lookup is ambiguous or incomplete",
            )),
        }
    }
    pub(crate) fn create(&self, name: &str) -> Result<String, ProtocolError> {
        let mut data=self.0.call("mutation PulsarCreateTag($input: TagCreateInput!) { tagCreate(input: $input) { id name } }",json!({"input":{"name":name}}))?;
        let tag: RawTag = serde_json::from_value(data["tagCreate"].take())
            .map_err(|_| unavailable("Stash tag creation acknowledgement is invalid"))?;
        self.tag(tag, name)
    }
    pub(crate) fn scene(&self, id: &str) -> Result<TaggedScene, ProtocolError> {
        if !self.0.safe_id(id) {
            return Err(ProtocolError::invalid("invalid Stash scene ID"));
        }
        let mut data=self.0.call("query PulsarTaggedScene($id: ID!) { findScene(id: $id) { id title interactive files { id path } tags { id } } }",json!({"id":id}))?;
        let raw = data["findScene"].take();
        let tags = raw["tags"]
            .as_array()
            .ok_or_else(|| unavailable("Stash scene lacks tag list"))?;
        if tags.len() > 512 {
            return Err(unavailable("Stash scene tag list exceeds limit"));
        }
        let mut ids = Vec::new();
        let mut unique = BTreeSet::new();
        for tag in tags {
            let id = tag["id"]
                .as_str()
                .ok_or_else(|| unavailable("invalid Stash scene tag ID"))?;
            if !self.0.safe_id(id) || !unique.insert(id) {
                return Err(unavailable("unsafe or duplicate Stash scene tag ID"));
            }
            ids.push(id.to_owned());
        }
        let scene = self.0.checked_scene(
            serde_json::from_value(raw).map_err(|_| unavailable("invalid tagged Stash scene"))?,
        )?;
        if scene.id != id {
            return Err(unavailable("tagged Stash scene identity mismatch"));
        }
        Ok(TaggedScene { scene, tags: ids })
    }
    pub(crate) fn add(&self, scene: &str, tag: &str) -> Result<(), ProtocolError> {
        if !self.0.safe_id(scene) || !self.0.safe_id(tag) {
            return Err(ProtocolError::invalid("invalid Stash scene/tag ID"));
        }
        let data=self.0.call("mutation PulsarAddTag($input: BulkSceneUpdateInput!) { bulkSceneUpdate(input: $input) { id } }",json!({"input":{"ids":[scene],"tag_ids":{"ids":[tag],"mode":"ADD"}}}))?;
        let rows = data["bulkSceneUpdate"]
            .as_array()
            .ok_or_else(|| unavailable("Stash ADD acknowledgement is invalid"))?;
        if rows.len() != 1 || rows[0]["id"].as_str() != Some(scene) {
            return Err(unavailable(
                "Stash ADD acknowledgement has a different scene identity",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tag_tests {
    use super::*;
    #[test]
    fn exact_tag_lookup_rejects_case_drift_duplicates_and_incomplete_results() {
        let responses = [
            json!({"data":{"findTags":{"count":0,"tags":[]}}}),
            json!({"data":{"findTags":{"count":1,"tags":[{"id":"tag","name":"Authored"}]}}}),
            json!({"data":{"findTags":{"count":1,"tags":[{"id":"tag","name":"authored"}]}}}),
            json!({"data":{"findTags":{"count":2,"tags":[{"id":"one","name":"Authored"},{"id":"two","name":"Authored"}]}}}),
            json!({"data":{"findTags":{"count":1,"tags":[]}}}),
        ];
        let (connection, server) = super::tests::mock(
            responses
                .into_iter()
                .map(|v| (200, String::new(), v.to_string()))
                .collect(),
        );
        let broker = TagBroker::new(&connection).unwrap();
        assert_eq!(broker.lookup("Authored").unwrap(), None);
        assert_eq!(broker.lookup("Authored").unwrap(), Some("tag".into()));
        for _ in 0..3 {
            assert!(broker.lookup("Authored").is_err());
        }
        let requests = server.join().unwrap();
        assert_eq!(requests[0]["variables"]["name"], "Authored");
        assert!(requests[0]["query"]
            .as_str()
            .unwrap()
            .contains("modifier: EQUALS"));
    }
    #[test]
    fn tag_add_is_single_scene_add_and_requires_matching_acknowledgement() {
        let values = [
            json!({"data":{"bulkSceneUpdate":[{"id":"scene"}]}}),
            json!({"data":{"bulkSceneUpdate":[{"id":"other"}]}}),
            json!({"data":{"bulkSceneUpdate":null}}),
        ];
        let (connection, server) = super::tests::mock(
            values
                .into_iter()
                .map(|v| (200, String::new(), v.to_string()))
                .collect(),
        );
        let broker = TagBroker::new(&connection).unwrap();
        broker.add("scene", "tag").unwrap();
        assert!(broker.add("scene", "tag").is_err());
        assert!(broker.add("scene", "tag").is_err());
        for request in server.join().unwrap() {
            assert_eq!(
                request["variables"]["input"],
                json!({"ids":["scene"],"tag_ids":{"ids":["tag"],"mode":"ADD"}})
            );
        }
    }
    #[test]
    fn creation_acknowledgement_is_named_identity_and_never_boolean_success() {
        let values = [
            json!({"data":{"tagCreate":{"id":"tag","name":"Authored"}}}),
            json!({"data":{"tagCreate":true}}),
            json!({"data":{"tagCreate":{"id":"tag","name":"Other"}}}),
        ];
        let (connection, server) = super::tests::mock(
            values
                .into_iter()
                .map(|v| (200, String::new(), v.to_string()))
                .collect(),
        );
        let broker = TagBroker::new(&connection).unwrap();
        assert_eq!(broker.create("Authored").unwrap(), "tag");
        assert!(broker.create("Authored").is_err());
        assert!(broker.create("Authored").is_err());
        assert_eq!(server.join().unwrap().len(), 3);
    }
}
