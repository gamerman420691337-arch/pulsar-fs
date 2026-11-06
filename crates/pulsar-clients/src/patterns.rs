//! Protocol-only authoring and checked submission of explicit axis patterns.
use crate::{
    get_snapshot, modality::await_candidate, session::EngineApi, ProjectSnapshot, ResponseBody,
};
use anyhow::{ensure, Context, Result};
use pulsar_protocol::*;
use std::path::Path;
const MAX_BYTES: usize = 6 * 4096;
/// Shared CLI/desktop builder: one strict ASCII key=value line per unique axis.
pub fn build_pattern_input(text: &str) -> Result<GenerationInput> {
    ensure!(
        text.len() <= MAX_BYTES && text.is_ascii(),
        "axis patterns require at most 24 KiB of ASCII text"
    );
    let lines: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(7)
        .collect();
    ensure!(
        !lines.is_empty() && lines.len() <= Axis::ALL.len(),
        "axis patterns require one through six nonempty lines"
    );
    let patterns = lines
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            parse_pattern_prompt(line)
                .with_context(|| format!("invalid axis pattern line {}", i + 1))
        })
        .collect::<Result<Vec<_>>>()?;
    let input = GenerationInput::Patterns { patterns };
    input.validate()?;
    Ok(input)
}
pub(crate) fn read_pattern_input(path: &Path) -> Result<GenerationInput> {
    let bytes = crate::inference::read_bounded_profile(path)?;
    let text = std::str::from_utf8(&bytes).context("axis pattern file must be UTF-8 ASCII text")?;
    build_pattern_input(text)
}
/// Revision-bound request construction is shared with the desktop controller.
pub(crate) fn candidate_command(input: GenerationInput) -> Result<Command> {
    ensure!(
        matches!(input, GenerationInput::Patterns { .. }),
        "expected explicit axis patterns"
    );
    input.validate()?;
    Ok(Command::GenerateInput { input })
}
pub(crate) fn generate_candidate(
    api: &mut impl EngineApi,
    project: ProjectSnapshot,
    revision: RevisionId,
    input: GenerationInput,
) -> Result<crate::CandidateSnapshot> {
    let command = candidate_command(input)?;
    ensure!(
        project.revision == revision,
        "pattern generation requires the current explicitly named revision"
    );
    let job = match api.execute(command, Some(project.project_id.clone()), Some(revision))? {
        ResponseBody::Job(job) => job,
        _ => anyhow::bail!("unexpected pattern generation response"),
    };
    let candidate = await_candidate(api, &project, job)?;
    ensure!(
        candidate.project_id == project.project_id && candidate.base_revision == revision,
        "pattern candidate requires explicit rebase and fresh review"
    );
    Ok(candidate)
}
pub(crate) fn stage(
    api: &mut impl EngineApi,
    project: &str,
    revision: RevisionId,
    input: GenerationInput,
) -> Result<()> {
    let snapshot = get_snapshot(api, &ProjectId::new(project)?)?;
    let candidate = generate_candidate(api, snapshot, revision, input)?;
    eprintln!("Explicit axis patterns are synthesized. Review the candidate and commit separately; master and devices are unchanged.");
    crate::print_response(ResponseBody::Candidate(Box::new(candidate)))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn hold(axis: &str, seconds: u32) -> String {
        format!("pattern=hold axis={axis} duration={seconds}s frequency=0hz amplitude=0 offset=0.4")
    }
    #[test]
    fn shared_builder_preserves_six_axes_exact_parameters_and_explicit_generation_request() {
        let text = ["stroke", "sway", "surge", "roll", "pitch", "yaw"]
            .map(|axis| hold(axis, 2))
            .join("\n");
        let input = build_pattern_input(&text).unwrap();
        let GenerationInput::Patterns { patterns } = &input else {
            panic!()
        };
        assert_eq!(patterns.len(), 6);
        assert_eq!(
            patterns.iter().map(|p| p.axis()).collect::<Vec<_>>(),
            Axis::ALL.to_vec()
        );
        assert!(patterns
            .iter()
            .all(|p| p.parameters().duration.as_nanos() == 2_000_000_000
                && p.parameters().offset.value() == 0.4));
        assert!(matches!(
            candidate_command(input).unwrap(),
            Command::GenerateInput {
                input: GenerationInput::Patterns { .. }
            }
        ));
    }
    #[test]
    fn malformed_duplicate_mismatched_duration_and_oversized_families_refuse_before_submission() {
        for text in [
            String::new(),
            hold("stroke", 1) + "\n" + &hold("stroke", 1),
            hold("stroke", 1) + "\n" + &hold("yaw", 2),
            "pattern=hold axis=stroke".into(),
            "x".repeat(MAX_BYTES + 1),
            hold("stroke", 1) + "\u{2003}",
            std::iter::repeat_n(hold("stroke", 1), 7)
                .collect::<Vec<_>>()
                .join("\n"),
        ] {
            assert!(build_pattern_input(&text).is_err());
        }
        assert!(candidate_command(GenerationInput::Text {
            prompt: hold("stroke", 1)
        })
        .is_err());
    }
    #[test]
    fn aggregate_action_budget_rejects_six_individually_valid_large_patterns() {
        let text=["stroke","sway","surge","roll","pitch","yaw"].map(|axis|format!("pattern=triangle axis={axis} duration=1000s frequency=1hz amplitude=0.1 offset=0.5 sample_hz=200")).join("\n");
        assert!(build_pattern_input(&text).is_err());
    }
}
