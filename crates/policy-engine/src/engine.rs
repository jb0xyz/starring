use serde::{Deserialize, Serialize};

use operation_graph::OperationGraph;

use crate::finding::Finding;
use crate::rule::PolicyRule;
use crate::verdict::Verdict;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyDecision {
    pub verdict: Verdict,
    pub findings: Vec<Finding>,
}

pub struct PolicyEngine {
    rules: Vec<Box<dyn PolicyRule + Send + Sync>>,
}

impl PolicyEngine {
    pub fn new(rules: Vec<Box<dyn PolicyRule + Send + Sync>>) -> Self {
        Self { rules }
    }

    pub fn evaluate(&self, graph: &OperationGraph) -> PolicyDecision {
        let mut findings = Vec::new();
        for rule in &self.rules {
            findings.extend(rule.evaluate(graph));
        }
        let verdict = findings
            .iter()
            .map(|finding| finding.verdict)
            .max()
            .unwrap_or(Verdict::Allow);
        PolicyDecision { verdict, findings }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::Finding;
    use crate::rule::PolicyRule;
    use crate::verdict::Verdict;
    use operation_graph::OperationGraph;

    struct MockRule(Verdict);

    impl PolicyRule for MockRule {
        fn id(&self) -> &str {
            "mock"
        }

        fn evaluate(&self, _graph: &OperationGraph) -> Vec<Finding> {
            vec![Finding {
                rule_id: "mock".to_string(),
                verdict: self.0,
                target: "t".to_string(),
                message: "m".to_string(),
            }]
        }
    }

    #[test]
    fn verdict_ordering() {
        assert!(Verdict::Deny > Verdict::RequireApproval);
        assert!(Verdict::RequireApproval > Verdict::Warn);
        assert!(Verdict::Warn > Verdict::Allow);
    }

    #[test]
    fn empty_engine_allows() {
        let engine = PolicyEngine::new(vec![]);
        assert_eq!(
            engine.evaluate(&OperationGraph::default()).verdict,
            Verdict::Allow
        );
    }

    #[test]
    fn aggregates_max_verdict() {
        let engine = PolicyEngine::new(vec![
            Box::new(MockRule(Verdict::Warn)),
            Box::new(MockRule(Verdict::Deny)),
        ]);
        let decision = engine.evaluate(&OperationGraph::default());
        assert_eq!(decision.verdict, Verdict::Deny);
        assert_eq!(decision.findings.len(), 2);
    }
}
