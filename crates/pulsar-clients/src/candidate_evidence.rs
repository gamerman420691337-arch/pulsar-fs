//! Presentation of labels already carried by hydrated candidate motion.
use pulsar_protocol::{Axis, EvidenceKind, MotionProgram};
use std::sync::{Arc, Weak};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Row {
    pub axis: Axis,
    pub observed: usize,
    pub predicted: usize,
    pub inferred: usize,
    pub synthesized: usize,
    pub gaps: usize,
}

#[derive(Default)]
pub(super) struct SummaryCache {
    program: Weak<MotionProgram>,
    rows: Vec<Row>,
}
impl SummaryCache {
    pub fn rows(&mut self, program: &Arc<MotionProgram>) -> &[Row] {
        let current = Arc::downgrade(program);
        if !self.program.ptr_eq(&current) {
            self.rows = summarize(program);
            self.program = current;
        }
        &self.rows
    }
}

fn summarize(program: &MotionProgram) -> Vec<Row> {
    program
        .tracks()
        .iter()
        .map(|track| {
            let mut row = Row {
                axis: track.axis(),
                observed: 0,
                predicted: 0,
                inferred: 0,
                synthesized: 0,
                gaps: track.gaps().len(),
            };
            for action in track.actions() {
                match action.evidence() {
                    EvidenceKind::Observed => row.observed += 1,
                    EvidenceKind::Predicted => row.predicted += 1,
                    EvidenceKind::Inferred => row.inferred += 1,
                    EvidenceKind::Synthesized => row.synthesized += 1,
                    // Unavailable evidence is represented by gaps; validated
                    // MotionAction cannot contain this label.
                    EvidenceKind::Unavailable => {}
                }
            }
            row
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulsar_protocol::{MotionAction, MotionTrack, NormalizedPosition, ProjectTime, TimeRange};

    fn action(ns: i64, evidence: EvidenceKind) -> MotionAction {
        MotionAction::new(
            ProjectTime::from_nanos(ns),
            NormalizedPosition::new(0.5).unwrap(),
            evidence,
        )
        .unwrap()
    }

    #[test]
    fn mixed_labels_and_gap_only_axes_stay_distinct() {
        let program = MotionProgram::new(vec![
            MotionTrack::new(
                Axis::Stroke,
                vec![
                    action(0, EvidenceKind::Observed),
                    action(1, EvidenceKind::Predicted),
                    action(2, EvidenceKind::Inferred),
                    action(3, EvidenceKind::Synthesized),
                    action(4, EvidenceKind::Synthesized),
                ],
            )
            .unwrap(),
            MotionTrack::with_gaps(
                Axis::Yaw,
                vec![],
                vec![TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(10)).unwrap()],
            )
            .unwrap(),
        ])
        .unwrap();
        let rows = summarize(&program);
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0],
            Row {
                axis: Axis::Stroke,
                observed: 1,
                predicted: 1,
                inferred: 1,
                synthesized: 2,
                gaps: 0
            }
        );
        assert_eq!(
            rows[1],
            Row {
                axis: Axis::Yaw,
                observed: 0,
                predicted: 0,
                inferred: 0,
                synthesized: 0,
                gaps: 1
            }
        );
    }

    #[test]
    fn changed_program_replaces_cached_labels_without_retaining_motion() {
        let mut cache = SummaryCache::default();
        let program = |evidence| {
            Arc::new(
                MotionProgram::new(vec![MotionTrack::new(
                    Axis::Stroke,
                    vec![action(0, evidence)],
                )
                .unwrap()])
                .unwrap(),
            )
        };
        let first = program(EvidenceKind::Synthesized);
        let owner = Arc::downgrade(&first);
        assert_eq!(cache.rows(&first)[0].synthesized, 1);
        let stored = cache.rows(&first).as_ptr();
        assert_eq!(cache.rows(&Arc::clone(&first)).as_ptr(), stored);
        assert_eq!(Arc::strong_count(&first), 1);
        drop(first);
        assert!(owner.upgrade().is_none());
        let second = program(EvidenceKind::Observed);
        assert_eq!(cache.rows(&second)[0].observed, 1);
        assert_eq!(cache.rows(&second)[0].synthesized, 0);
        assert!(cache
            .rows(&Arc::new(MotionProgram::new(vec![]).unwrap()))
            .is_empty());
    }
}
