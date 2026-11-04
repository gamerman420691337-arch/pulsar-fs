//! Shared local script preparation. It never uploads or contacts a device.
use crate::{motion::ResponseBody, session::EngineApi};
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use std::path::PathBuf;
pub fn prepare(
    client: &mut impl EngineApi,
    project: ProjectId,
    revision: RevisionId,
    path: PathBuf,
    id: RequestId,
) -> Result<HandyStrokeScript> {
    let response=client.execute_identified(id.clone(),Command::PrepareHandyStrokeScript {path:path.clone()},Some(project.clone()),Some(revision))
        .with_context(||format!("Handy preparation {id}; retain its exact project, revision and destination for recovery"))?;
    let ResponseBody::HandyStrokeScript(receipt) = response else {
        anyhow::bail!("Handy preparation response mismatch");
    };
    receipt.validate()?;
    ensure!(
        receipt.request_id == id
            && receipt.project_id == project
            && receipt.revision == revision
            && receipt.path == path,
        "Handy preparation receipt changed its captured intent"
    );
    Ok(*receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Api {
        reply: Option<ResponseBody>,
        calls: Vec<(RequestId, Command, Option<ProjectId>, Option<RevisionId>)>,
    }
    impl EngineApi for Api {
        fn execute(
            &mut self,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            anyhow::bail!("uncaptured request")
        }
        fn execute_identified(
            &mut self,
            id: RequestId,
            c: Command,
            p: Option<ProjectId>,
            r: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            self.calls.push((id, c, p, r));
            self.reply
                .take()
                .ok_or_else(|| anyhow::anyhow!("synthetic lost reply"))
        }
    }
    #[test]
    fn handy_script_client_preserves_original_intent_and_rejects_changed_receipt() {
        let p = ProjectId::new("p").unwrap();
        let r = RevisionId::new(2);
        let path = std::env::temp_dir().join("stroke.csv");
        let id = RequestId::new("original").unwrap();
        let mut api = Api {
            reply: None,
            calls: vec![],
        };
        assert!(prepare(&mut api, p.clone(), r, path.clone(), id.clone()).is_err());
        let receipt = HandyStrokeScript {
            request_id: id.clone(),
            project_id: p.clone(),
            revision: r,
            path: path.clone(),
            master_sha256: "a".repeat(64),
            identity: ArtifactIdentity {
                artifact_id: ArtifactId::new("b".repeat(64)).unwrap(),
                sha256: "b".repeat(64),
                byte_len: 40,
            },
            quantization: HandyStrokeQuantization {
                action_count: 2,
                first_time_ms: 0,
                last_time_ms: 10,
                max_time_error_ns: 0,
                max_position_error_ppm: 0,
                omitted_axes: vec![],
            },
            unresolved_review_count: 1,
            review_state_unknown: false,
            release_qualified: false,
        };
        api.reply = Some(ResponseBody::HandyStrokeScript(Box::new(receipt.clone())));
        assert_eq!(
            prepare(&mut api, p.clone(), r, path.clone(), id.clone())
                .unwrap()
                .identity,
            receipt.identity
        );
        assert_eq!(
            serde_json::to_value(&api.calls[0]).unwrap(),
            serde_json::to_value(&api.calls[1]).unwrap()
        );
        let mut changed = receipt.clone();
        changed.revision = RevisionId::new(3);
        api.reply = Some(ResponseBody::HandyStrokeScript(Box::new(changed)));
        assert!(prepare(&mut api, p.clone(), r, path.clone(), id.clone()).is_err());
        let mut qualified = receipt;
        qualified.release_qualified = true;
        api.reply = Some(ResponseBody::HandyStrokeScript(Box::new(qualified)));
        assert!(prepare(&mut api, p, r, path, id).is_err());
    }
}
