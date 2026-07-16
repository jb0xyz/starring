#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
use super::super::intent_interpretation::IntentBoundaryRequestV2;
#[cfg(test)]
use super::classification::SECRET_TARGETS;
#[cfg(test)]
use super::syntax::TextSpan;

mod canonical;
mod coverage;
mod grouping;

pub(super) use canonical::{unique_visible_bounded_span, CanonicalWhitespaceMap};
pub(super) use grouping::{
    coordinated_groups_cover_candidate, cross_sentence_role_bridge, evidence_groups,
    BoundaryEvidenceGroup,
};

#[cfg(test)]
use coverage::{cover_markers, covered_runs, marker_occurrence_spans, unsafe_secret_target_spans};
#[cfg(test)]
use grouping::merged_spans;

#[cfg(test)]
thread_local! {
    static COMPONENT_ONLY_EVALUATIONS: Cell<usize> = const { Cell::new(0) };
    static POLARITY_PREFIX_CHAR_VISITS: Cell<usize> = const { Cell::new(0) };
    static EVIDENCE_SPAN_LINEAR_WORK: Cell<usize> = const { Cell::new(0) };
    static MERGED_SPAN_LOCAL_WORK: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
mod tests {
    use super::super::classification::classify_sentence_units;
    use super::super::syntax::sentence_spans;
    use super::*;

    fn component_evaluations(unit_count: usize) -> usize {
        let human = vec!["skip validation"; unit_count].join(" and ");
        let visible = human.chars().collect::<Vec<_>>();
        let (span, question) = sentence_spans(&visible).into_iter().next().unwrap();
        let units = classify_sentence_units(&visible, span, question);
        assert_eq!(units.len(), unit_count);
        COMPONENT_ONLY_EVALUATIONS.with(|evaluations| evaluations.set(0));
        let groups = evidence_groups(&units, &visible);
        assert_eq!(groups.len(), 1);
        assert_eq!(
            groups[0].request(),
            IntentBoundaryRequestV2::BypassValidationPreviewApproval
        );
        COMPONENT_ONLY_EVALUATIONS.with(Cell::get)
    }

    fn polarity_prefix_visits(repetitions: usize) -> usize {
        let human = vec!["skip validation"; repetitions].join(" ");
        let visible = human.chars().collect::<Vec<_>>();
        POLARITY_PREFIX_CHAR_VISITS.with(|visits| visits.set(0));
        let spans = marker_occurrence_spans(
            &visible,
            TextSpan {
                start: 0,
                end: visible.len(),
            },
            &["skip"],
            true,
        );
        assert_eq!(spans.len(), repetitions);
        POLARITY_PREFIX_CHAR_VISITS.with(Cell::get)
    }

    #[test]
    fn component_classification_work_scales_linearly() {
        let small = component_evaluations(128);
        let large = component_evaluations(256);
        assert_eq!(small, 128);
        assert_eq!(large, 256);
        assert_eq!(large, small * 2);
    }

    #[test]
    fn action_polarity_prefix_work_uses_a_fixed_token_window() {
        let small = polarity_prefix_visits(128);
        let large = polarity_prefix_visits(256);
        assert!(
            large <= small.saturating_mul(2).saturating_add(2048),
            "small {small} large {large}"
        );

        for (controls, owned) in [(15, false), (16, true), (17, true)] {
            let human = format!("{}skip validation", "never ".repeat(controls));
            let visible = human.chars().collect::<Vec<_>>();
            let spans = marker_occurrence_spans(
                &visible,
                TextSpan {
                    start: 0,
                    end: visible.len(),
                },
                &["skip"],
                true,
            );
            assert_eq!(!spans.is_empty(), owned, "control count {controls}");
        }
    }

    #[test]
    fn marker_scans_preserve_ascii_folding_unicode_and_boundaries() {
        let human = "SKIP 검증 skipx Ä ä";
        let visible = human.chars().collect::<Vec<_>>();
        let span = TextSpan {
            start: 0,
            end: visible.len(),
        };
        let expected = vec!["SKIP", "검증", "Ä"];

        let spans = marker_occurrence_spans(&visible, span, &["skip", "검증", "Ä"], false);
        let matched = spans
            .iter()
            .map(|span| visible[span.start..span.end].iter().collect::<String>())
            .collect::<Vec<_>>();
        assert_eq!(matched, expected);

        let mut covered = vec![false; visible.len()];
        cover_markers(&visible, &mut covered, &["skip", "검증", "Ä"]);
        let covered = covered_runs(&covered, 0)
            .iter()
            .map(|span| visible[span.start..span.end].iter().collect::<String>())
            .collect::<Vec<_>>();
        assert_eq!(covered, expected);
    }

    #[test]
    fn secret_evidence_spans_measure_the_full_linear_path() {
        fn work(repeated: usize) -> usize {
            let human = "Publish the API token publicly. ".repeat(repeated);
            let visible = human.chars().collect::<Vec<_>>();
            EVIDENCE_SPAN_LINEAR_WORK.with(|steps| steps.set(0));
            let spans = unsafe_secret_target_spans(
                &visible,
                TextSpan {
                    start: 0,
                    end: visible.len(),
                },
            );
            assert_eq!(spans.len(), repeated);
            EVIDENCE_SPAN_LINEAR_WORK.with(Cell::get)
        }

        let small = work(256);
        let large = work(512);
        let fixed_marker_cost = SECRET_TARGETS
            .iter()
            .map(|marker| marker.chars().count())
            .sum::<usize>();
        assert!(
            large <= small.saturating_mul(2).saturating_add(fixed_marker_cost),
            "small {small} large {large}"
        );
    }

    #[test]
    fn merged_evidence_spans_measure_the_full_linear_path() {
        fn work(repeated: usize) -> usize {
            let spans = (0..repeated)
                .map(|index| TextSpan {
                    start: index.saturating_mul(2),
                    end: index.saturating_mul(2).saturating_add(1),
                })
                .collect::<Vec<_>>();
            EVIDENCE_SPAN_LINEAR_WORK.with(|steps| steps.set(0));
            assert_eq!(merged_spans(spans).len(), repeated);
            EVIDENCE_SPAN_LINEAR_WORK.with(Cell::get)
        }

        let small = work(2_048);
        let large = work(4_096);
        assert!(large <= small.saturating_mul(2).saturating_add(2));
    }

    #[test]
    fn separated_boundary_groups_use_group_local_merge_ranges() {
        fn work(repeated: usize) -> usize {
            let human = "Skip validation. ".repeat(repeated);
            MERGED_SPAN_LOCAL_WORK.with(|steps| steps.set(0));
            let analysis = super::super::SafetyBoundaryAnalysis::analyze(&human);
            assert_eq!(
                analysis.requests(),
                &[IntentBoundaryRequestV2::BypassValidationPreviewApproval]
            );
            MERGED_SPAN_LOCAL_WORK.with(Cell::get)
        }

        let small = work(1_024);
        let large = work(2_048);
        assert!(small > 0);
        assert!(large <= small.saturating_mul(2).saturating_add(32));
    }
}
