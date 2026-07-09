use operation_graph::OperationGraph;

use crate::finding::Finding;

pub trait PolicyRule {
    fn id(&self) -> &str;
    fn evaluate(&self, graph: &OperationGraph) -> Vec<Finding>;
}
