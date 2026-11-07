//! Device-control session gate: one user toggle, bounded and revocable authority.
//!
//! A session is the standing grant described by spec E.7. While it is open the
//! assistant may move the device without per-action approval; when it closes, expires
//! or the device is lost, motion stops and nothing restores it without a fresh toggle.
//!
//! This module is pure. It performs no I/O, holds no clock of its own and never
//! contacts a device. Callers pass `now_ns`. Stop and disarm are unconditional and
//! idempotent: they must succeed even when the model is mid-stream, unresponsive or
//! absent, so they are not modelled as refusable transitions.
use pulsar_protocol::{ContentClass, ErrorCode, ProjectId, ProtocolError};
use serde::{Deserialize, Serialize};

/// Hard ceiling on a session, independent of what a caller asks for.
pub const MAX_DEVICE_SESSION_LIFETIME_NS: u64 = 60 * 60 * 1_000_000_000;
/// Default lifetime proposed by DCS-08.
pub const DEFAULT_DEVICE_SESSION_LIFETIME_NS: u64 = 20 * 60 * 1_000_000_000;
/// Default amplitude ceiling proposed by DCS-08, in percent of usable travel.
pub const DEFAULT_AMPLITUDE_CAP_PERCENT: u8 = 70;

/// Who is attempting an action on the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionAuthority {
    /// The local user surface: the only authority that may open, renew or extend.
    UserSurface,
    /// A tool, model, remote provider or automation. May submit motion only.
    Tool,
}

/// Limits fixed at session creation. Motion is reduced to fit, never widened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceMotionCaps {
    /// Amplitude ceiling as a percentage of full excursion about the midpoint.
    pub amplitude_percent: u8,
    /// Lowest usable device position, 0-100.
    pub min_position: u8,
    /// Highest usable device position, 0-100.
    pub max_position: u8,
    /// Ceiling on travel rate in position units per second. Zero means unthrottled.
    pub max_rate_per_second: u16,
}

impl DeviceMotionCaps {
    pub fn new(
        amplitude_percent: u8,
        min_position: u8,
        max_position: u8,
        max_rate_per_second: u16,
    ) -> Result<Self, ProtocolError> {
        let caps = Self {
            amplitude_percent,
            min_position,
            max_position,
            max_rate_per_second,
        };
        caps.validate()?;
        Ok(caps)
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        if !(1..=100).contains(&self.amplitude_percent)
            || self.min_position >= self.max_position
            || self.max_position > 100
        {
            return Err(ProtocolError::invalid("device motion cap bounds differ"));
        }
        Ok(())
    }
}

impl Default for DeviceMotionCaps {
    fn default() -> Self {
        Self {
            amplitude_percent: DEFAULT_AMPLITUDE_CAP_PERCENT,
            min_position: 0,
            max_position: 100,
            max_rate_per_second: 0,
        }
    }
}

/// Why a session action was refused. Labels are stable and carry no content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionRefusal {
    /// No session exists for this device and owner.
    Absent,
    /// The session reached its lifetime bound.
    Expired,
    /// The session was stopped or disarmed.
    Stopped,
    /// A tool tried to open, renew or extend.
    UserOnly,
    /// The session belongs to another owner, project or device.
    Scope,
    /// The requested lifetime is not admissible.
    Lifetime,
    /// The session's content class does not admit the request.
    ContentClass,
}

impl SessionRefusal {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Absent => "session_absent",
            Self::Expired => "session_expired",
            Self::Stopped => "session_stopped",
            Self::UserOnly => "session_user_only",
            Self::Scope => "session_scope",
            Self::Lifetime => "session_lifetime",
            Self::ContentClass => "session_content_class",
        }
    }

    pub fn error(self) -> ProtocolError {
        let code = match self {
            Self::UserOnly => ErrorCode::Forbidden,
            Self::Absent => ErrorCode::NotFound,
            _ => ErrorCode::RequestConflict,
        };
        ProtocolError::new(code, format!("device control session: {}", self.label()))
    }
}

/// One position submitted by a tool, after the session's caps were applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LimitedMotion {
    /// The position that may actually be dispatched.
    pub position: u8,
    /// True when the request was reduced rather than executed as asked.
    pub limited: bool,
}

/// A bounded grant to move one device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceControlSession {
    owner: String,
    project: ProjectId,
    device: String,
    content_class: ContentClass,
    caps: DeviceMotionCaps,
    opened_at_ns: u64,
    expires_at_ns: u64,
    stopped: bool,
    last: Option<(u64, u8)>,
}

impl DeviceControlSession {
    /// Open a session. Only the user surface may do this.
    pub fn open(
        authority: SessionAuthority,
        owner: impl Into<String>,
        project: ProjectId,
        device: impl Into<String>,
        content_class: ContentClass,
        caps: DeviceMotionCaps,
        lifetime_ns: u64,
        now_ns: u64,
    ) -> Result<Self, SessionRefusal> {
        if authority != SessionAuthority::UserSurface {
            return Err(SessionRefusal::UserOnly);
        }
        let device = device.into();
        if lifetime_ns == 0 || lifetime_ns > MAX_DEVICE_SESSION_LIFETIME_NS {
            return Err(SessionRefusal::Lifetime);
        }
        caps.validate().map_err(|_| SessionRefusal::Lifetime)?;
        let expires_at_ns = now_ns.saturating_add(lifetime_ns);
        Ok(Self {
            owner: owner.into(),
            project,
            device,
            content_class,
            caps,
            opened_at_ns: now_ns,
            expires_at_ns,
            stopped: false,
            last: None,
        })
    }

    pub fn caps(&self) -> DeviceMotionCaps {
        self.caps
    }
    pub fn device(&self) -> &str {
        &self.device
    }
    pub fn content_class(&self) -> ContentClass {
        self.content_class
    }
    pub fn opened_at_ns(&self) -> u64 {
        self.opened_at_ns
    }
    pub fn expires_at_ns(&self) -> u64 {
        self.expires_at_ns
    }
    pub fn remaining_ns(&self, now_ns: u64) -> u64 {
        self.expires_at_ns.saturating_sub(now_ns)
    }
    pub fn is_stopped(&self) -> bool {
        self.stopped
    }

    /// Whether motion may be dispatched now.
    pub fn admits(&self, now_ns: u64) -> bool {
        !self.stopped && now_ns < self.expires_at_ns
    }

    fn check(&self, now_ns: u64) -> Result<(), SessionRefusal> {
        if self.stopped {
            return Err(SessionRefusal::Stopped);
        }
        if now_ns >= self.expires_at_ns {
            return Err(SessionRefusal::Expired);
        }
        Ok(())
    }

    /// Confirm the request targets this exact owner/project/device/class.
    pub fn check_scope(
        &self,
        owner: &str,
        project: &ProjectId,
        device: &str,
        content_class: ContentClass,
        now_ns: u64,
    ) -> Result<(), SessionRefusal> {
        self.check(now_ns)?;
        if self.owner != owner
            || &self.project != project
            || self.device != device
            || self.content_class != content_class
        {
            return Err(SessionRefusal::Scope);
        }
        Ok(())
    }

    /// Submit one position. Reduced to fit the caps; never widened.
    pub fn submit(&mut self, requested: u8, now_ns: u64) -> Result<LimitedMotion, SessionRefusal> {
        self.check(now_ns)?;
        if requested > 100 {
            return Err(SessionRefusal::Scope);
        }
        let amplitude = i32::from(self.caps.amplitude_percent);
        // Reduce excursion about the midpoint, then confine to usable travel.
        let centred = 50 + ((i32::from(requested) - 50) * amplitude) / 100;
        let confined = centred.clamp(
            i32::from(self.caps.min_position),
            i32::from(self.caps.max_position),
        );
        let mut position = u8::try_from(confined).unwrap_or(50);
        let mut limited = position != requested;

        // Rate limiting is applied against the previous dispatched position.
        if let (true, Some((last_ns, last_position))) =
            (self.caps.max_rate_per_second > 0, self.last)
        {
            let elapsed_ns = now_ns.saturating_sub(last_ns);
            let allowed = (u64::from(self.caps.max_rate_per_second) * elapsed_ns / 1_000_000_000)
                .min(100) as i32;
            let delta = i32::from(position) - i32::from(last_position);
            if delta.abs() > allowed {
                let clamped = i32::from(last_position) + delta.signum() * allowed;
                position = u8::try_from(clamped.clamp(0, 100)).unwrap_or(last_position);
                limited = true;
            }
        }

        self.last = Some((now_ns, position));
        Ok(LimitedMotion { position, limited })
    }

    /// End the session. Unconditional and idempotent.
    pub fn stop(&mut self) {
        self.stopped = true;
        self.last = None;
    }

    /// Extend a live session. Only the user surface may renew, and an expired or
    /// stopped session cannot be revived — that requires a fresh toggle.
    pub fn renew(
        &mut self,
        authority: SessionAuthority,
        lifetime_ns: u64,
        now_ns: u64,
    ) -> Result<(), SessionRefusal> {
        if authority != SessionAuthority::UserSurface {
            return Err(SessionRefusal::UserOnly);
        }
        self.check(now_ns)?;
        if lifetime_ns == 0 || lifetime_ns > MAX_DEVICE_SESSION_LIFETIME_NS {
            return Err(SessionRefusal::Lifetime);
        }
        self.expires_at_ns = self.expires_at_ns.max(now_ns.saturating_add(lifetime_ns));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> ProjectId {
        ProjectId::new("project").unwrap()
    }

    fn open(now: u64) -> DeviceControlSession {
        DeviceControlSession::open(
            SessionAuthority::UserSurface,
            "owner",
            project(),
            "handy-1",
            ContentClass::Explicit,
            DeviceMotionCaps::default(),
            DEFAULT_DEVICE_SESSION_LIFETIME_NS,
            now,
        )
        .unwrap()
    }

    #[test]
    fn only_the_user_surface_may_open_a_session() {
        let error = DeviceControlSession::open(
            SessionAuthority::Tool,
            "owner",
            project(),
            "handy-1",
            ContentClass::General,
            DeviceMotionCaps::default(),
            DEFAULT_DEVICE_SESSION_LIFETIME_NS,
            0,
        )
        .unwrap_err();
        assert_eq!(error, SessionRefusal::UserOnly);
        assert_eq!(error.error().code, ErrorCode::Forbidden);
    }

    #[test]
    fn a_session_expires_without_user_action() {
        let mut session = open(0);
        assert!(session.admits(1_000));
        let expiry = session.expires_at_ns();
        // The default 70% cap contracts 30 toward the midpoint: 50 + (30-50)*0.7.
        let capped = session.submit(30, expiry - 1).unwrap();
        assert_eq!(capped.position, 36);
        assert!(capped.limited);
        assert_eq!(session.submit(30, expiry), Err(SessionRefusal::Expired));
        assert!(!session.admits(expiry));
    }

    #[test]
    fn a_lifetime_beyond_the_hard_ceiling_is_refused() {
        let error = DeviceControlSession::open(
            SessionAuthority::UserSurface,
            "owner",
            project(),
            "handy-1",
            ContentClass::General,
            DeviceMotionCaps::default(),
            MAX_DEVICE_SESSION_LIFETIME_NS + 1,
            0,
        )
        .unwrap_err();
        assert_eq!(error, SessionRefusal::Lifetime);
    }

    #[test]
    fn stop_is_unconditional_idempotent_and_blocks_motion() {
        let mut session = open(0);
        session.stop();
        session.stop();
        assert!(session.is_stopped());
        assert!(!session.admits(10));
        assert_eq!(session.submit(50, 10), Err(SessionRefusal::Stopped));
    }

    #[test]
    fn a_stopped_session_cannot_be_renewed_back_to_life() {
        let mut session = open(0);
        session.stop();
        assert_eq!(
            session.renew(
                SessionAuthority::UserSurface,
                DEFAULT_DEVICE_SESSION_LIFETIME_NS,
                10
            ),
            Err(SessionRefusal::Stopped)
        );
        assert!(!session.admits(10));
    }

    #[test]
    fn expiry_cannot_be_renewed_only_reopened() {
        let mut session = open(0);
        let expiry = session.expires_at_ns();
        assert_eq!(
            session.renew(
                SessionAuthority::UserSurface,
                DEFAULT_DEVICE_SESSION_LIFETIME_NS,
                expiry
            ),
            Err(SessionRefusal::Expired)
        );
    }

    #[test]
    fn a_tool_cannot_renew_or_extend_a_session() {
        let mut session = open(0);
        assert_eq!(
            session.renew(
                SessionAuthority::Tool,
                DEFAULT_DEVICE_SESSION_LIFETIME_NS,
                10
            ),
            Err(SessionRefusal::UserOnly)
        );
    }

    #[test]
    fn the_default_cap_reduces_excursion_and_says_so() {
        let mut session = open(0);
        // Midpoint passes through unchanged and is not reported as limited.
        let middle = session.submit(50, 10).unwrap();
        assert_eq!(
            middle,
            LimitedMotion {
                position: 50,
                limited: false
            }
        );
        // Full travel is reduced to the 70% cap: 100 -> 50 + 35.
        let top = session.submit(100, 20).unwrap();
        assert_eq!(
            top,
            LimitedMotion {
                position: 85,
                limited: true
            }
        );
        let bottom = session.submit(0, 30).unwrap();
        assert_eq!(
            bottom,
            LimitedMotion {
                position: 15,
                limited: true
            }
        );
    }

    #[test]
    fn usable_travel_confines_the_result() {
        let caps = DeviceMotionCaps::new(100, 20, 60, 0).unwrap();
        let mut session = DeviceControlSession::open(
            SessionAuthority::UserSurface,
            "owner",
            project(),
            "handy-1",
            ContentClass::General,
            caps,
            DEFAULT_DEVICE_SESSION_LIFETIME_NS,
            0,
        )
        .unwrap();
        assert_eq!(session.submit(100, 10).unwrap().position, 60);
        assert_eq!(session.submit(0, 20).unwrap().position, 20);
        assert!(session.submit(100, 30).unwrap().limited);
    }

    #[test]
    fn rate_limiting_bounds_travel_between_submissions() {
        let caps = DeviceMotionCaps::new(100, 0, 100, 20).unwrap();
        let mut session = DeviceControlSession::open(
            SessionAuthority::UserSurface,
            "owner",
            project(),
            "handy-1",
            ContentClass::General,
            caps,
            MAX_DEVICE_SESSION_LIFETIME_NS,
            0,
        )
        .unwrap();
        assert_eq!(session.submit(50, 0).unwrap().position, 50);
        // 20 units/second over 1 second allows 10 units of travel from 50.
        let limited = session.submit(100, 1_000_000_000).unwrap();
        assert_eq!(limited.position, 70);
        assert!(limited.limited);
        // A full second more admits the rest.
        assert_eq!(session.submit(100, 2_000_000_000).unwrap().position, 90);
    }

    #[test]
    fn scope_mismatch_is_refused() {
        let session = open(0);
        assert!(session
            .check_scope("owner", &project(), "handy-1", ContentClass::Explicit, 10)
            .is_ok());
        for (owner, device, class) in [
            ("other", "handy-1", ContentClass::Explicit),
            ("owner", "handy-2", ContentClass::Explicit),
            ("owner", "handy-1", ContentClass::General),
        ] {
            assert_eq!(
                session.check_scope(owner, &project(), device, class, 10),
                Err(SessionRefusal::Scope)
            );
        }
        let other_project = ProjectId::new("other-project").unwrap();
        assert_eq!(
            session.check_scope(
                "owner",
                &other_project,
                "handy-1",
                ContentClass::Explicit,
                10
            ),
            Err(SessionRefusal::Scope)
        );
    }

    #[test]
    fn out_of_range_positions_are_refused_rather_than_clamped() {
        let mut session = open(0);
        assert_eq!(session.submit(101, 10), Err(SessionRefusal::Scope));
    }

    #[test]
    fn remaining_time_is_reported_for_the_visible_control() {
        let session = open(0);
        assert_eq!(session.remaining_ns(0), DEFAULT_DEVICE_SESSION_LIFETIME_NS);
        assert_eq!(session.remaining_ns(DEFAULT_DEVICE_SESSION_LIFETIME_NS), 0);
    }

    #[test]
    fn renewal_extends_but_never_shortens() {
        let mut session = open(0);
        let original = session.expires_at_ns();
        session
            .renew(
                SessionAuthority::UserSurface,
                DEFAULT_DEVICE_SESSION_LIFETIME_NS,
                10,
            )
            .unwrap();
        assert!(session.expires_at_ns() >= original);
        // A shorter renewal request does not pull the expiry inward.
        let extended = session.expires_at_ns();
        session
            .renew(SessionAuthority::UserSurface, 1_000_000_000, 20)
            .unwrap();
        assert_eq!(session.expires_at_ns(), extended);
    }
}
