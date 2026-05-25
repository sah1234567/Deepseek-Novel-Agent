use crate::KnowledgeError;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::{Bfs, EdgeRef, Walker};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CausalityNode {
    pub id: String,
    pub description: String,
    pub chapter: String,
}

pub struct CausalityGraph {
    graph: DiGraph<CausalityNode, String>,
    id_index: HashMap<String, NodeIndex>,
}

impl CausalityGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            id_index: HashMap::new(),
        }
    }

    pub fn add_edge(
        &mut self,
        from: CausalityNode,
        to: CausalityNode,
        relation: &str,
    ) -> Result<(), KnowledgeError> {
        let from_idx = self.ensure_node(from);
        let to_idx = self.ensure_node(to);
        if would_create_cycle(&self.graph, from_idx, to_idx) {
            return Err(KnowledgeError::CycleDetected(format!(
                "{} -> {}",
                self.graph[from_idx].id, self.graph[to_idx].id
            )));
        }
        self.graph.add_edge(from_idx, to_idx, relation.to_string());
        Ok(())
    }

    fn ensure_node(&mut self, node: CausalityNode) -> NodeIndex {
        if let Some(&idx) = self.id_index.get(&node.id) {
            return idx;
        }
        let idx = self.graph.add_node(node.clone());
        self.id_index.insert(node.id, idx);
        idx
    }

    pub fn traverse_forward(&self, event_id: &str, depth: usize) -> Vec<CausalityNode> {
        self.traverse(event_id, depth, true)
    }

    pub fn traverse_backward(&self, event_id: &str, depth: usize) -> Vec<CausalityNode> {
        self.traverse(event_id, depth, false)
    }

    fn traverse(&self, event_id: &str, depth: usize, forward: bool) -> Vec<CausalityNode> {
        let Some(&start) = self.id_index.get(event_id) else {
            return vec![];
        };
        let mut visited = HashSet::new();
        let mut out = Vec::new();
        let mut queue = vec![(start, 0usize)];
        visited.insert(start);
        while let Some((node, d)) = queue.pop() {
            if d > 0 {
                out.push(self.graph[node].clone());
            }
            if d >= depth {
                continue;
            }
            let neighbors: Vec<NodeIndex> = if forward {
                self.graph.edges(node).map(|e| e.target()).collect()
            } else {
                self.graph
                    .edges_directed(node, petgraph::Direction::Incoming)
                    .map(|e| e.source())
                    .collect()
            };
            for n in neighbors {
                if visited.insert(n) {
                    queue.push((n, d + 1));
                }
            }
        }
        out
    }
}

impl Default for CausalityGraph {
    fn default() -> Self {
        Self::new()
    }
}

fn would_create_cycle(g: &DiGraph<CausalityNode, String>, from: NodeIndex, to: NodeIndex) -> bool {
    if from == to {
        return true;
    }
    let bfs = Bfs::new(g, to);
    bfs.iter(g).any(|n| n == from)
}

/// Parse markdown causality table rows into graph edges.
pub fn parse_causality_markdown(content: &str) -> CausalityGraph {
    let mut graph = CausalityGraph::new();
    for line in content.lines() {
        let line = line.trim();
        if !line.starts_with('|') || line.contains("---") || line.contains("章节") {
            continue;
        }
        let cols: Vec<&str> = line
            .split('|')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if cols.len() < 5 {
            continue;
        }
        let chapter = cols[0].to_string();
        let from_id = cols[1].to_string();
        let _relation = cols[2];
        let to_id = cols[3].to_string();
        let desc = cols[4].to_string();
        if from_id == "—" || from_id.is_empty() {
            continue;
        }
        let _ = graph.add_edge(
            CausalityNode {
                id: from_id.clone(),
                description: desc.clone(),
                chapter: chapter.clone(),
            },
            CausalityNode {
                id: to_id,
                description: desc,
                chapter,
            },
            "edge",
        );
    }
    graph
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[test]
    fn traverse_forward_and_backward() {
        let mut g = CausalityGraph::new();
        g.add_edge(
            CausalityNode {
                id: "E05".into(),
                description: "误入禁地".into(),
                chapter: "Ch5".into(),
            },
            CausalityNode {
                id: "E06".into(),
                description: "发现石碑".into(),
                chapter: "Ch5".into(),
            },
            "导致",
        )
        .unwrap();
        g.add_edge(
            CausalityNode {
                id: "E06".into(),
                description: "发现石碑".into(),
                chapter: "Ch5".into(),
            },
            CausalityNode {
                id: "E07".into(),
                description: "血脉异动".into(),
                chapter: "Ch5".into(),
            },
            "触发",
        )
        .unwrap();
        assert_eq!(g.traverse_forward("E05", 3).len(), 2);
        assert_eq!(g.traverse_backward("E07", 3).len(), 2);
    }

    #[rstest]
    #[test]
    fn cycle_detection() {
        let mut g = CausalityGraph::new();
        g.add_edge(
            CausalityNode {
                id: "A".into(),
                description: "a".into(),
                chapter: "Ch1".into(),
            },
            CausalityNode {
                id: "B".into(),
                description: "b".into(),
                chapter: "Ch1".into(),
            },
            "r",
        )
        .unwrap();
        let err = g.add_edge(
            CausalityNode {
                id: "B".into(),
                description: "b".into(),
                chapter: "Ch1".into(),
            },
            CausalityNode {
                id: "A".into(),
                description: "a".into(),
                chapter: "Ch1".into(),
            },
            "r",
        );
        assert!(matches!(err, Err(KnowledgeError::CycleDetected(_))));
    }
}
