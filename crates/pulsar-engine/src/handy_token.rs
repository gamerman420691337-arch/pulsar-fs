//! Handy v3 client-token custody.
//!
//! Handy REST v3 separates two credentials:
//!
//! * the **ApplicationKey** (`X-Api-Key`) authenticates only the privileged
//!   `/auth/token/issue` endpoint, and
//! * a short-lived **client token** (`Authorization: Bearer`) authenticates the
//!   non-privileged device endpoints, always alongside `X-Connection-Key`.
//!
//! The ApplicationKey must never be sent to a device endpoint. This module mints
//! and caches the client token, and hands out a bearer value only while it is
//! still valid with a conservative margin.
use crate::handy_credentials::Credentials;
use pulsar_protocol::*;
use std::{
    io::Read,
    sync::Mutex,
    time::{Duration, Instant},
};

pub(crate) const TOKEN_ORIGIN: &str = "https://www.handyfeeling.com/api/handy-rest/v3";
const ISSUE_TIMEOUT: Duration = Duration::from_secs(10);
const RESPONSE_LIMIT: u64 = 64 * 1024;
/// Renew this long before the declared expiry so an in-flight command cannot
/// cross the boundary with a token that expires mid-request.
const EXPIRY_MARGIN: Duration = Duration::from_secs(120);
const MIN_ISSUED_TTL: Duration = Duration::from_secs(60);

fn unavailable(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::Unavailable, message)
}

#[derive(Debug)]
struct CachedToken {
    token: String,
    expires_at: Instant,
}

/// Serialized token custody for one credential pair.
///
/// One live token is retained at a time. Acquisition is serialized so concurrent
/// commands cannot issue duplicate tokens, and an expired entry is always
/// replaced rather than reused.
pub(crate) struct TokenBroker {
    cached: Mutex<Option<CachedToken>>,
}

impl TokenBroker {
    pub(crate) fn new() -> Self {
        Self {
            cached: Mutex::new(None),
        }
    }

    /// Return a currently valid bearer token, issuing one when none is cached or
    /// the cached value is inside its expiry margin.
    pub(crate) fn bearer(&self, credentials: &Credentials) -> Result<String, ProtocolError> {
        let mut guard = self
            .cached
            .lock()
            .map_err(|_| unavailable("Handy token custody is unavailable"))?;
        if let Some(cached) = guard.as_ref() {
            if Instant::now() + EXPIRY_MARGIN < cached.expires_at {
                return Ok(cached.token.clone());
            }
        }
        let issued = issue(credentials, ISSUE_TIMEOUT)?;
        let token = issued.token.clone();
        *guard = Some(issued);
        Ok(token)
    }

    /// Discard the cached token so the next acquisition issues a fresh one. Used
    /// after a device endpoint reports the token as unauthorized.
    pub(crate) fn invalidate(&self) {
        if let Ok(mut guard) = self.cached.lock() {
            *guard = None;
        }
    }

    #[cfg(test)]
    pub(crate) fn seed_for_test(&self, token: &str, expires_at: Instant) {
        *self.cached.lock().unwrap() = Some(CachedToken {
            token: token.to_owned(),
            expires_at,
        });
    }
}

/// Issue one client token bound to this device's connection key.
fn issue(credentials: &Credentials, timeout: Duration) -> Result<CachedToken, ProtocolError> {
    let started = Instant::now();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .max_redirects(0)
        .proxy(None)
        .build()
        .into();
    let mut request = agent
        .get(format!(
            "{TOKEN_ORIGIN}/auth/token/issue?to={}&ttl=3600",
            credentials.connection
        ))
        .header("X-Api-Key", &credentials.api)
        .header("Accept", "application/json");
    if let Some(value) = request.headers_mut().and_then(|h| h.get_mut("X-Api-Key")) {
        value.set_sensitive(true);
    }
    let mut response = request
        .call()
        .map_err(|_| unavailable("Handy token issuance failed; remote detail is withheld"))?;
    if response.status().as_u16() != 200 {
        return Err(unavailable(
            "Handy token issuance returned a non-success HTTP status",
        ));
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(RESPONSE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unavailable("Handy token response read failed"))?;
    if bytes.len() as u64 > RESPONSE_LIMIT {
        return Err(unavailable("Handy token response exceeds 64 KiB"));
    }
    let token = parse_token(&bytes)?;
    // Retain the shorter of the declared lifetime and the remaining request
    // budget, so a slow issue cannot create a token we already consider stale.
    let elapsed = started.elapsed();
    let expires_at = (started + token.ttl)
        .checked_sub(elapsed)
        .ok_or_else(|| unavailable("Handy token lifetime underflow"))?;
    if expires_at.saturating_duration_since(Instant::now()) < MIN_ISSUED_TTL {
        return Err(unavailable("Handy token lifetime is too short"));
    }
    Ok(CachedToken {
        token: token.value,
        expires_at,
    })
}

struct ParsedToken {
    value: String,
    ttl: Duration,
}

fn parse_token(bytes: &[u8]) -> Result<ParsedToken, ProtocolError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| unavailable("invalid Handy token response"))?;
    let result = value
        .get("result")
        .ok_or_else(|| unavailable("Handy token response lacks a result"))?;
    let token = result
        .get("token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| unavailable("Handy token response lacks a token"))?;
    // Structural checks only: this establishes a usable header value, not
    // server-verified authorization.
    if token.is_empty() || token.len() > 4096 || !token.bytes().all(|c| (0x21..=0x7e).contains(&c))
    {
        return Err(unavailable("invalid Handy client token"));
    }
    let ttl = match result.get("expires_at").and_then(|v| v.as_str()) {
        Some(text) => declared_ttl(text, result.get("created_at").and_then(|v| v.as_str()))?,
        None => Duration::from_secs(3600),
    };
    Ok(ParsedToken {
        value: token.to_owned(),
        ttl,
    })
}

/// Derive a lifetime from the RFC 3339 `created_at`/`expires_at` pair. A
/// missing or unparseable pair falls back to the requested one-hour ttl rather
/// than trusting an unbounded value.
fn declared_ttl(expires_at: &str, created_at: Option<&str>) -> Result<Duration, ProtocolError> {
    let parse = |text: &str| {
        // Minute-resolution fallback parse: "YYYY-MM-DDTHH:MM:SS".
        let bytes = text.as_bytes();
        if bytes.len() < 19 {
            return None;
        }
        let num = |s: &str| s.parse::<i64>().ok();
        let year = num(&text[0..4])?;
        let month = num(&text[5..7])?;
        let day = num(&text[8..10])?;
        let hour = num(&text[11..13])?;
        let minute = num(&text[14..16])?;
        let second = num(&text[17..19])?;
        Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second)
    };
    let end = parse(expires_at).ok_or_else(|| unavailable("invalid Handy token expiry"))?;
    let start = created_at.and_then(parse).unwrap_or(end - 3_600);
    let seconds = end - start;
    if !(60..=86_400 + 60).contains(&seconds) {
        return Err(unavailable(
            "Handy token lifetime is outside the declared range",
        ));
    }
    Ok(Duration::from_secs(seconds.max(60) as u64))
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credentials() -> Credentials {
        Credentials::for_test("application-key", "connection-key")
    }

    #[test]
    fn parses_issued_token_and_declared_lifetime() {
        let body = serde_json::json!({
            "result": {
                "token": "issued-token-value",
                "created_at": "2026-10-29T01:29:55.411258Z",
                "expires_at": "2026-10-29T02:29:55.395424565Z"
            }
        })
        .to_string();
        let parsed = parse_token(body.as_bytes()).unwrap();
        assert_eq!(parsed.value, "issued-token-value");
        assert_eq!(parsed.ttl, Duration::from_secs(3_600));
    }

    #[test]
    fn rejects_malformed_or_unsafe_tokens() {
        for body in [
            serde_json::json!({"result": {}}),
            serde_json::json!({"result": {"token": ""}}),
            serde_json::json!({"result": {"token": "bad\ntoken"}}),
            serde_json::json!({}),
        ] {
            assert!(
                parse_token(body.to_string().as_bytes()).is_err(),
                "accepted invalid token response {body}"
            );
        }
        assert!(parse_token(b"not json").is_err());
    }

    #[test]
    fn cached_token_is_reused_then_replaced_inside_the_expiry_margin() {
        let broker = TokenBroker::new();
        broker.seed_for_test("live", Instant::now() + Duration::from_secs(3_600));
        assert_eq!(broker.bearer(&credentials()).unwrap(), "live");
        assert_eq!(
            broker.bearer(&credentials()).unwrap(),
            "live",
            "a valid token must not be re-issued"
        );

        // Inside the margin the cached value is no longer returned; issuance is
        // attempted instead (and fails here without network authority).
        broker.seed_for_test("stale", Instant::now() + Duration::from_secs(1));
        assert!(broker.bearer(&credentials()).is_err());

        // Invalidation clears custody.
        broker.seed_for_test("live", Instant::now() + Duration::from_secs(3_600));
        broker.invalidate();
        assert!(broker.bearer(&credentials()).is_err());
    }

    #[test]
    fn civil_day_arithmetic_matches_known_dates() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(
            days_from_civil(2026, 10, 29) - days_from_civil(2026, 10, 28),
            1
        );
    }
}
