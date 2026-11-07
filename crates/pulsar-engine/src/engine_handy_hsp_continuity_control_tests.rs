// Included in authority::tests. No native network or worker is admitted here.
mod hsp_continuity_control_tests {
    use super::*;

    fn enable(f: &Fixture, stream: RequestId, consent: bool) -> Request {
        request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::EnableHandyHspContinuity {
                stream_request_id: stream,
                duration_ms: 1_000,
                allow_online_observations: consent,
                owner_proof: f.engine.handy_owner_token.clone(),
            },
        )
    }
    fn stored(f: &Fixture, request: &Request) -> u64 {
        f.engine
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM requests WHERE session=?1 AND request=?2",
                params![f.session.as_str(), request.request_id.as_str()],
                |row| row.get(0),
            )
            .unwrap()
    }
    #[test]
    fn continuity_refuses_absent_source_without_reserving_or_starting_owned_work() {
        let f = fixture();
        let request = enable(&f, RequestId::new("absent-live-stream").unwrap(), true);
        let error = f.engine.handle(request.clone()).result.unwrap_err();
        assert_eq!(error.code, ErrorCode::NotFound);
        assert_eq!(stored(&f, &request), 0);
        assert!(f.engine.handy_hsp_execution.lock().unwrap().is_none());
        f.engine.finish_shutdown().unwrap();
    }
    #[test]
    fn continuity_consent_refusal_precedes_history_and_native_admission() {
        let f = fixture();
        let request = enable(
            &f,
            RequestId::new("unconsented-live-stream").unwrap(),
            false,
        );
        let error = f.engine.handle(request.clone()).result.unwrap_err();
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert_eq!(stored(&f, &request), 0);
        assert!(f.engine.handy_hsp_execution.lock().unwrap().is_none());
        f.engine.finish_shutdown().unwrap();
    }
    #[test]
    fn continuity_status_of_absent_original_cannot_restore_a_native_owner() {
        let f = fixture();
        let query = request(
            &f.session,
            Some(&f.project),
            None,
            Command::HandyHspContinuityStatus {
                original_request_id: RequestId::new("absent-enable-request").unwrap(),
                owner_proof: f.engine.handy_owner_token.clone(),
            },
        );
        assert_eq!(
            f.engine.handle(query.clone()).result.unwrap_err().code,
            ErrorCode::NotFound
        );
        assert_eq!(stored(&f, &query), 0);
        assert!(f.engine.handy_hsp_execution.lock().unwrap().is_none());
        f.engine.finish_shutdown().unwrap();
    }
}
