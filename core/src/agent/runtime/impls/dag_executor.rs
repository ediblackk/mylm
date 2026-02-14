//! DAG Executor - executes IntentGraph respecting dependencies
//!
//! The executor takes an IntentGraph and:
//! 1. Identifies ready intents (all dependencies satisfied)
//! 2. Executes them in parallel (up to concurrency limits)
//! 3. Returns observations when all complete

use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::agent::contract::{
    graph::IntentGraph,
    observations::{Observation, ExecutionSummary},
    ids::IntentId,
    AgencyRuntime,
    runtime::RuntimeError,
};

/// DAG execution result
#[derive(Debug)]
pub struct DagExecutionResult {
    /// Observations for each completed intent
    pub observations: Vec<(IntentId, Observation)>,
    /// Execution summary
    pub summary: ExecutionSummary,
    /// Any errors that occurred
    pub errors: Vec<(IntentId, RuntimeError)>,
}

/// Executes a DAG of intents respecting dependencies
pub async fn execute_dag<R: AgencyRuntime + 'static>(
    runtime: Arc<R>,
    graph: &IntentGraph,
    max_parallel: usize,
) -> Result<DagExecutionResult, RuntimeError> {
    let completed = Arc::new(Mutex::new(HashSet::<IntentId>::new()));
    let in_flight = Arc::new(Mutex::new(HashSet::<IntentId>::new()));
    let mut observations = Vec::new();
    let mut errors = Vec::new();

    loop {
        let completed_guard = completed.lock().await;
        let completed_count = completed_guard.len();
        let graph_len = graph.len();
        drop(completed_guard);

        // Check if all intents completed
        if completed_count >= graph_len {
            break;
        }

        // Get current state
        let completed_vec: Vec<_> = completed.lock().await.iter().copied().collect();
        let in_flight_count = in_flight.lock().await.len();
        let available_slots = max_parallel.saturating_sub(in_flight_count);

        // Get all ready nodes
        let all_ready: Vec<_> = graph
            .ready_nodes(&completed_vec)
            .into_iter()
            .cloned()
            .collect();

        // Filter out already in-flight or completed
        let mut ready = Vec::new();
        for node in all_ready {
            let inflight_guard = in_flight.lock().await;
            let completed_guard = completed.lock().await;
            if !inflight_guard.contains(&node.id) && !completed_guard.contains(&node.id) {
                ready.push(node);
            }
            if ready.len() >= available_slots {
                break;
            }
        }

        if ready.is_empty() && in_flight_count == 0 {
            // Deadlock or all done
            break;
        }

        // Execute ready intents concurrently
        let mut handles = Vec::new();
        for node in ready {
            let runtime = runtime.clone();
            let in_flight = in_flight.clone();
            
            in_flight.lock().await.insert(node.id);
            
            let handle = tokio::spawn(async move {
                let result = runtime.execute_with_id(node.id, node.intent.clone()).await;
                (node.id, result)
            });
            handles.push(handle);
        }

        // Wait for all current batch to complete
        for handle in handles {
            match handle.await {
                Ok((id, Ok(obs))) => {
                    completed.lock().await.insert(id);
                    in_flight.lock().await.remove(&id);
                    observations.push((id, obs));
                }
                Ok((id, Err(e))) => {
                    in_flight.lock().await.remove(&id);
                    errors.push((id, e));
                }
                Err(e) => {
                    return Err(RuntimeError::Internal {
                        message: format!("Task panic: {:?}", e),
                    });
                }
            }
        }
    }

    let summary = ExecutionSummary::from_observations(
        &observations.iter().map(|(_, o)| o.clone()).collect::<Vec<_>>()
    );

    Ok(DagExecutionResult {
        observations,
        summary,
        errors,
    })
}

/// Simple DAG executor struct for convenience
pub struct DagExecutor;

impl DagExecutor {
    /// Execute a DAG with default parallelism
    pub async fn execute<R: AgencyRuntime + 'static>(
        runtime: Arc<R>,
        graph: &IntentGraph,
    ) -> Result<DagExecutionResult, RuntimeError> {
        execute_dag(runtime, graph, 10).await
    }

    /// Execute a DAG with custom parallelism
    pub async fn execute_with_parallelism<R: AgencyRuntime + 'static>(
        runtime: Arc<R>,
        graph: &IntentGraph,
        max_parallel: usize,
    ) -> Result<DagExecutionResult, RuntimeError> {
        execute_dag(runtime, graph, max_parallel).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::contract::{
        intents::Intent,
        graph::IntentGraphBuilder,
        ids::IntentId,
        events::ToolResult,
    };
    use async_trait::async_trait;

    struct MockRuntime;

    #[async_trait]
    impl AgencyRuntime for MockRuntime {
        async fn execute(&self, intent: Intent) -> Result<Observation, RuntimeError> {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            
            match intent {
                Intent::EmitResponse(text) => {
                    Ok(Observation::ResponseEmitted {
                        intent_id: IntentId::new(1),
                        content: text,
                        is_partial: false,
                    })
                }
                _ => Ok(Observation::ToolCompleted {
                    intent_id: IntentId::new(1),
                    result: ToolResult::Success {
                        output: "ok".to_string(),
                        structured: None,
                    },
                    execution_time_ms: 10,
                }),
            }
        }

        async fn execute_dag(
            &self,
            _graph: &IntentGraph,
        ) -> Result<Vec<(IntentId, Observation)>, RuntimeError> {
            // Simple stub - just return empty results
            Ok(Vec::new())
        }

        fn subscribe_telemetry(&self) -> tokio::sync::broadcast::Receiver<crate::agent::contract::runtime::TelemetryEvent> {
            let (tx, _) = tokio::sync::broadcast::channel(1);
            tx.subscribe()
        }

        async fn health_check(&self) -> crate::agent::contract::runtime::HealthStatus {
            crate::agent::contract::runtime::HealthStatus::Healthy
        }

        async fn shutdown(&self) -> Result<(), RuntimeError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_sequential_execution() {
        let runtime = Arc::new(MockRuntime);

        let mut builder = IntentGraphBuilder::at_step(1);
        let a = builder.add(Intent::EmitResponse("A".to_string()));
        let b = builder.add_with_deps(Intent::EmitResponse("B".to_string()), vec![a]);
        let _c = builder.add_with_deps(Intent::EmitResponse("C".to_string()), vec![b]);

        let graph = builder.build();
        let result = DagExecutor::execute(runtime, &graph).await.unwrap();

        assert_eq!(result.observations.len(), 3);
    }

    #[tokio::test]
    async fn test_parallel_execution() {
        let runtime = Arc::new(MockRuntime);

        let mut builder = IntentGraphBuilder::at_step(1);
        let _a = builder.add(Intent::EmitResponse("A".to_string()));
        let _b = builder.add(Intent::EmitResponse("B".to_string()));
        let _c = builder.add(Intent::EmitResponse("C".to_string()));

        let graph = builder.build();
        let start = std::time::Instant::now();
        let result = DagExecutor::execute(runtime, &graph).await.unwrap();
        let elapsed = start.elapsed();

        assert_eq!(result.observations.len(), 3);
        assert!(elapsed < std::time::Duration::from_millis(50), 
            "Parallel execution took too long: {:?}", elapsed);
    }
}
