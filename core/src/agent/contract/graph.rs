//! IntentGraph - DAG structure for batch intent processing
//!
//! The kernel emits a graph of intents with dependencies.
//! The runtime executes them respecting the dependency structure.

use std::collections::{HashMap, HashSet};

use super::ids::IntentId;
use super::intents::{IntentNode, Intent};
use super::ContractError;

/// A directed acyclic graph of intents
/// 
/// This is what the kernel emits from process().
/// The runtime consumes this and executes intents in dependency order.
#[derive(Debug, Clone, Default)]
pub struct IntentGraph {
    /// All nodes in the graph
    nodes: HashMap<IntentId, IntentNode>,
    /// Cached topological order (computed on first access)
    topological_order: Option<Vec<IntentId>>,
}

impl IntentGraph {
    /// Create an empty graph
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            topological_order: None,
        }
    }

    /// Create a graph from a single intent
    pub fn single(id: IntentId, intent: Intent) -> Self {
        let mut graph = Self::new();
        graph.add(IntentNode::new(id, intent));
        graph
    }

    /// Add a node to the graph
    pub fn add(&mut self, node: IntentNode) {
        self.nodes.insert(node.id, node);
        self.topological_order = None; // Invalidate cache
    }

    /// Add multiple nodes
    pub fn add_nodes(&mut self, nodes: impl IntoIterator<Item = IntentNode>) {
        for node in nodes {
            self.add(node);
        }
    }

    /// Get a node by ID
    pub fn get(&self, id: IntentId) -> Option<&IntentNode> {
        self.nodes.get(&id)
    }

    /// Get a mutable node by ID
    pub fn get_mut(&mut self, id: IntentId) -> Option<&mut IntentNode> {
        self.topological_order = None;
        self.nodes.get_mut(&id)
    }

    /// Check if graph contains a node
    pub fn contains(&self, id: IntentId) -> bool {
        self.nodes.contains_key(&id)
    }

    /// Number of nodes in the graph
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Check if graph is empty
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Get all nodes
    pub fn nodes(&self) -> impl Iterator<Item = &IntentNode> {
        self.nodes.values()
    }

    /// Get all node IDs
    pub fn node_ids(&self) -> impl Iterator<Item = IntentId> + '_ {
        self.nodes.keys().copied()
    }

    /// Get nodes that are ready to execute (all dependencies met)
    pub fn ready_nodes(&self, completed: &[IntentId]) -> Vec<&IntentNode> {
        let completed_set: HashSet<_> = completed.iter().copied().collect();
        
        self.nodes
            .values()
            .filter(|node| !completed_set.contains(&node.id))
            .filter(|node| node.dependencies.iter().all(|dep| completed_set.contains(dep)))
            .collect()
    }

    /// Get IDs of ready nodes
    pub fn ready_ids(&self, completed: &[IntentId]) -> Vec<IntentId> {
        self.ready_nodes(completed)
            .into_iter()
            .map(|n| n.id)
            .collect()
    }

    /// Check if a specific node is ready
    pub fn is_ready(&self, id: IntentId, completed: &[IntentId]) -> bool {
        let completed_set: HashSet<_> = completed.iter().copied().collect();
        
        if let Some(node) = self.nodes.get(&id) {
            !completed_set.contains(&id)
                && node.dependencies.iter().all(|dep| completed_set.contains(dep))
        } else {
            false
        }
    }

    /// Check if the graph is complete (all nodes completed)
    pub fn is_complete(&self, completed: &[IntentId]) -> bool {
        let completed_set: HashSet<_> = completed.iter().copied().collect();
        self.nodes.keys().all(|id| completed_set.contains(id))
    }

    /// Get completion percentage
    pub fn completion_ratio(&self, completed: &[IntentId]) -> f64 {
        if self.nodes.is_empty() {
            return 1.0;
        }
        completed.len() as f64 / self.nodes.len() as f64
    }

    /// Get nodes that depend on a given node
    pub fn dependents(&self, id: IntentId) -> Vec<IntentId> {
        self.nodes
            .values()
            .filter(|node| node.dependencies.contains(&id))
            .map(|node| node.id)
            .collect()
    }

    /// Get all dependencies (transitive) for a node
    pub fn transitive_dependencies(&self, id: IntentId) -> HashSet<IntentId> {
        let mut deps = HashSet::new();
        let mut to_process = vec![id];
        
        while let Some(current) = to_process.pop() {
            if let Some(node) = self.nodes.get(&current) {
                for dep in &node.dependencies {
                    if deps.insert(*dep) {
                        to_process.push(*dep);
                    }
                }
            }
        }
        
        deps
    }

    /// Check for cycles in the graph
    pub fn has_cycles(&self) -> bool {
        // Try to compute topological sort - if it fails, there's a cycle
        self.compute_topological_order().is_err()
    }

    /// Validate the graph (no cycles, all dependencies exist)
    pub fn validate(&self) -> Result<(), ContractError> {
        // Check all dependencies exist
        for node in self.nodes.values() {
            for dep in &node.dependencies {
                if !self.nodes.contains_key(dep) {
                    return Err(ContractError::UnknownDependency(*dep));
                }
            }
        }

        // Check for cycles
        if let Err(cycle) = self.compute_topological_order() {
            return Err(ContractError::CyclicDependency(cycle));
        }

        Ok(())
    }

    /// Get topological order of all nodes
    /// 
    /// Returns nodes in an order where dependencies come before dependents
    pub fn topological_order(&mut self) -> Result<&[IntentId], ContractError> {
        if self.topological_order.is_none() {
            let order = self.compute_topological_order()
                .map_err(|cycle| ContractError::CyclicDependency(cycle))?;
            self.topological_order = Some(order);
        }
        Ok(self.topological_order.as_ref().unwrap())
    }

    /// Compute topological order using Kahn's algorithm
    fn compute_topological_order(&self) -> Result<Vec<IntentId>, Vec<IntentId>> {
        let mut in_degree: HashMap<IntentId, usize> = HashMap::new();
        let mut adjacency: HashMap<IntentId, Vec<IntentId>> = HashMap::new();

        // Initialize in-degrees and adjacency
        for (&id, node) in &self.nodes {
            in_degree.entry(id).or_insert(0);
            for dep in &node.dependencies {
                adjacency.entry(*dep).or_default().push(id);
                *in_degree.entry(id).or_insert(0) += 1;
            }
        }

        // Start with nodes that have no dependencies
        let mut queue: Vec<IntentId> = in_degree
            .iter()
            .filter(|(_, &degree)| degree == 0)
            .map(|(&id, _)| id)
            .collect();

        let mut result = Vec::new();

        while let Some(id) = queue.pop() {
            result.push(id);

            if let Some(dependents) = adjacency.get(&id) {
                for &dependent in dependents {
                    if let Some(degree) = in_degree.get_mut(&dependent) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push(dependent);
                        }
                    }
                }
            }
        }

        // If not all nodes were processed, there's a cycle
        if result.len() != self.nodes.len() {
            // Find nodes in the cycle
            let remaining: Vec<IntentId> = in_degree
                .iter()
                .filter(|(_, &degree)| degree > 0)
                .map(|(&id, _)| id)
                .collect();
            return Err(remaining);
        }

        Ok(result)
    }

    /// Create a builder for constructing graphs at step 0
    /// 
    /// For deterministic IDs based on kernel state, use `IntentGraphBuilder::at_step(step_count)`.
    pub fn builder() -> IntentGraphBuilder {
        IntentGraphBuilder::at_step(0)
    }

    /// Merge another graph into this one
    pub fn merge(&mut self, other: IntentGraph) {
        for (_, node) in other.nodes {
            self.add(node);
        }
    }

    /// Get stats about the graph
    pub fn stats(&self) -> GraphStats {
        let total = self.nodes.len();
        let max_depth = self
            .nodes
            .keys()
            .map(|&id| self.transitive_dependencies(id).len())
            .max()
            .unwrap_or(0);

        GraphStats {
            total_nodes: total,
            max_dependency_depth: max_depth,
            root_nodes: self
                .nodes
                .values()
                .filter(|n| n.has_no_dependencies())
                .count(),
        }
    }
}

/// Builder for constructing IntentGraphs with deterministic IDs
/// 
/// Uses kernel's step_count to generate IntentIds deterministically.
/// Leader (Session) provides step_count. Workers never compute IDs.
#[derive(Debug)]
pub struct IntentGraphBuilder {
    graph: IntentGraph,
    step_count: u32,
    intent_index: u32,
}

impl IntentGraphBuilder {
    /// Create a new builder for a specific kernel step
    /// 
    /// # Arguments
    /// * `step_count` - Current kernel step count (from AgentState)
    /// 
    /// # Example
    /// ```
    /// // At kernel step 5
    /// let builder = IntentGraphBuilder::at_step(kernel.state().step_count as u32);
    /// ```
    pub fn at_step(step_count: u32) -> Self {
        Self {
            graph: IntentGraph::new(),
            step_count,
            intent_index: 0,
        }
    }

    /// Generate deterministic IntentId for current position
    fn next_id(&mut self) -> IntentId {
        let id = IntentId::from_step(self.step_count, self.intent_index);
        self.intent_index += 1;
        id
    }

    /// Add a node with deterministic ID
    pub fn add(&mut self, intent: Intent) -> IntentId {
        let id = self.next_id();
        self.graph.add(IntentNode::new(id, intent));
        id
    }

    /// Add a node with specific ID (only use if you know what you're doing)
    /// 
    /// Warning: Non-deterministic IDs break replay. Prefer `add()`.
    pub fn add_with_id(&mut self, id: IntentId, intent: Intent) -> &mut Self {
        self.graph.add(IntentNode::new(id, intent));
        self
    }

    /// Add a node with dependencies
    /// 
    /// Dependencies must reference IDs from this same builder (same step).
    pub fn add_with_deps(
        &mut self,
        intent: Intent,
        deps: Vec<IntentId>,
    ) -> IntentId {
        let id = self.next_id();
        
        let mut node = IntentNode::new(id, intent);
        for dep in deps {
            node.dependencies.push(dep);
        }
        self.graph.add(node);
        id
    }

    /// Chain: add a node that depends on the previous one
    pub fn then(&mut self, intent: Intent) -> IntentId {
        let prev_index = self.intent_index.saturating_sub(1);
        let prev_id = IntentId::from_step(self.step_count, prev_index);
        self.add_with_deps(intent, vec![prev_id])
    }

    /// Build the graph
    pub fn build(self) -> IntentGraph {
        self.graph
    }

    /// Build and validate
    pub fn build_validated(self) -> Result<IntentGraph, ContractError> {
        let graph = self.graph;
        graph.validate()?;
        Ok(graph)
    }

    /// Get current graph (for inspection during building)
    pub fn current(&self) -> &IntentGraph {
        &self.graph
    }

    /// Get mutable reference to a node
    pub fn node_mut(&mut self, id: IntentId) -> Option<&mut super::intents::IntentNode> {
        self.graph.get_mut(id)
    }

    /// Get the step count this builder is using
    pub fn step_count(&self) -> u32 {
        self.step_count
    }

    /// Get number of intents added so far
    pub fn intent_count(&self) -> u32 {
        self.intent_index
    }
}

impl Default for IntentGraphBuilder {
    fn default() -> Self {
        Self::at_step(0)
    }
}

/// Statistics about a graph
#[derive(Debug, Clone, Copy)]
pub struct GraphStats {
    pub total_nodes: usize,
    pub max_dependency_depth: usize,
    pub root_nodes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::intents::Intent;

    #[test]
    fn test_empty_graph() {
        let graph = IntentGraph::new();
        assert!(graph.is_empty());
        assert!(graph.is_complete(&[]));
    }

    #[test]
    fn test_single_node() {
        let mut graph = IntentGraph::new();
        graph.add(IntentNode::new(IntentId::new(1), Intent::Halt(super::super::intents::ExitReason::Completed)));
        
        assert_eq!(graph.len(), 1);
        assert!(graph.is_ready(IntentId::new(1), &[]));
    }

    #[test]
    fn test_dependencies() {
        let mut graph = IntentGraph::new();
        
        // Root node
        graph.add(IntentNode::new(IntentId::new(1), Intent::EmitResponse("A".to_string())));
        
        // Dependent node
        let mut node2 = IntentNode::new(IntentId::new(2), Intent::EmitResponse("B".to_string()));
        node2.dependencies.push(IntentId::new(1));
        graph.add(node2);
        
        // Check ready without completion
        let ready = graph.ready_ids(&[]);
        assert_eq!(ready, vec![IntentId::new(1)]);
        
        // Check ready with first completed
        let ready = graph.ready_ids(&[IntentId::new(1)]);
        assert_eq!(ready, vec![IntentId::new(2)]);
    }

    #[test]
    fn test_fan_out_fan_in() {
        let mut graph = IntentGraph::builder();
        
        // Fan out: A -> B, C, D (parallel)
        let a = graph.add(Intent::EmitResponse("A".to_string()));
        let b = graph.add_with_deps(Intent::EmitResponse("B".to_string()), vec![a]);
        let c = graph.add_with_deps(Intent::EmitResponse("C".to_string()), vec![a]);
        let d = graph.add_with_deps(Intent::EmitResponse("D".to_string()), vec![a]);
        
        // Fan in: B, C, D -> E
        let _e = graph.add_with_deps(Intent::EmitResponse("E".to_string()), vec![b, c, d]);
        
        let graph = graph.build();
        
        // Initially only A is ready
        assert!(graph.is_ready(a, &[]));
        assert!(!graph.is_ready(b, &[]));
        
        // After A, B C D are ready
        let ready = graph.ready_ids(&[a]);
        assert_eq!(ready.len(), 3);
    }

    #[test]
    fn test_cycle_detection() {
        let mut graph = IntentGraph::new();
        
        // Create a cycle: A -> B -> C -> A
        let mut node_a = IntentNode::new(IntentId::new(1), Intent::EmitResponse("A".to_string()));
        node_a.dependencies.push(IntentId::new(3)); // Depends on C
        
        let mut node_b = IntentNode::new(IntentId::new(2), Intent::EmitResponse("B".to_string()));
        node_b.dependencies.push(IntentId::new(1)); // Depends on A
        
        let mut node_c = IntentNode::new(IntentId::new(3), Intent::EmitResponse("C".to_string()));
        node_c.dependencies.push(IntentId::new(2)); // Depends on B
        
        graph.add(node_a);
        graph.add(node_b);
        graph.add(node_c);
        
        assert!(graph.has_cycles());
        assert!(graph.validate().is_err());
    }

    #[test]
    fn test_topological_order() {
        let mut graph = IntentGraph::builder();
        
        let a = graph.add(Intent::EmitResponse("A".to_string()));
        let b = graph.add_with_deps(Intent::EmitResponse("B".to_string()), vec![a]);
        let c = graph.add_with_deps(Intent::EmitResponse("C".to_string()), vec![a]);
        let d = graph.add_with_deps(Intent::EmitResponse("D".to_string()), vec![b, c]);
        
        let mut graph = graph.build();
        let order = graph.topological_order().unwrap();
        
        // A must come before B, C
        let pos_a = order.iter().position(|&id| id == a).unwrap();
        let pos_b = order.iter().position(|&id| id == b).unwrap();
        let pos_c = order.iter().position(|&id| id == c).unwrap();
        let pos_d = order.iter().position(|&id| id == d).unwrap();
        
        assert!(pos_a < pos_b);
        assert!(pos_a < pos_c);
        assert!(pos_b < pos_d);
        assert!(pos_c < pos_d);
    }
}
