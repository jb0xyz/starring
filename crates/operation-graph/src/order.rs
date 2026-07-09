use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use crate::error::OperationGraphError;
use crate::node::{OpId, OperationGraph};

impl OperationGraph {
    pub fn topological_order(&self) -> Result<Vec<OpId>, OperationGraphError> {
        let mut indegree: HashMap<OpId, usize> =
            self.nodes.iter().map(|node| (node.id, 0usize)).collect();
        let mut dependents: HashMap<OpId, Vec<OpId>> = HashMap::new();
        for node in &self.nodes {
            for dep in &node.depends_on {
                if let Some(d) = indegree.get_mut(&node.id) {
                    *d += 1;
                }
                dependents.entry(*dep).or_default().push(node.id);
            }
        }
        let mut heap: BinaryHeap<Reverse<OpId>> = self
            .nodes
            .iter()
            .filter(|node| indegree.get(&node.id).copied().unwrap_or(0) == 0)
            .map(|node| Reverse(node.id))
            .collect();
        let mut order = Vec::new();
        while let Some(Reverse(id)) = heap.pop() {
            order.push(id);
            if let Some(deps) = dependents.get(&id) {
                for dep in deps {
                    if let Some(e) = indegree.get_mut(dep) {
                        *e -= 1;
                        if *e == 0 {
                            heap.push(Reverse(*dep));
                        }
                    }
                }
            }
        }
        if order.len() == self.nodes.len() {
            Ok(order)
        } else {
            Err(OperationGraphError::DependencyCycle)
        }
    }

    pub fn rollback_order(&self) -> Result<Vec<OpId>, OperationGraphError> {
        let mut order = self.topological_order()?;
        order.reverse();
        Ok(order)
    }
}

#[cfg(test)]
mod tests {
    use crate::node::{OpId, Operation, OperationGraph, OperationNode};
    use desired_state::ResourceKey;

    fn node(id: u32, deps: Vec<u32>) -> OperationNode {
        OperationNode {
            id: OpId(id),
            operation: Operation::DeleteRole {
                key: ResourceKey(format!("k{id}")),
            },
            produces: vec![],
            consumes: vec![],
            depends_on: deps.into_iter().map(OpId).collect(),
        }
    }

    #[test]
    fn topo_and_rollback() {
        let g = OperationGraph {
            nodes: vec![node(0, vec![]), node(1, vec![0]), node(2, vec![1])],
        };
        assert_eq!(
            g.topological_order().unwrap(),
            vec![OpId(0), OpId(1), OpId(2)]
        );
        assert_eq!(g.rollback_order().unwrap(), vec![OpId(2), OpId(1), OpId(0)]);
    }

    #[test]
    fn cycle_detected() {
        let g = OperationGraph {
            nodes: vec![node(0, vec![1]), node(1, vec![0])],
        };
        assert!(g.topological_order().is_err());
    }
}
