use std::sync::{Arc, Mutex};
use std::time::Duration;

use prism_js::QueryPhaseView;
use serde_json::Value;

use crate::mcp_call_log::{duration_to_ms, summarize_value, touches_for_value};

tokio::task_local! {
    static CURRENT_RESOURCE_TRACE: ResourceTraceState;
}

#[derive(Clone, Default)]
pub(crate) struct ResourceTraceState {
    phases: Arc<Mutex<Vec<QueryPhaseView>>>,
}

#[derive(Clone, Default)]
pub(crate) struct ResourceTraceSnapshot {
    pub(crate) phases: Vec<QueryPhaseView>,
}

impl ResourceTraceState {
    pub(crate) async fn scope<Fut, T>(future: Fut) -> (T, ResourceTraceSnapshot)
    where
        Fut: std::future::Future<Output = T>,
    {
        let state = Self::default();
        let output = CURRENT_RESOURCE_TRACE.scope(state.clone(), future).await;
        (output, state.snapshot())
    }

    fn snapshot(&self) -> ResourceTraceSnapshot {
        ResourceTraceSnapshot {
            phases: self
                .phases
                .lock()
                .expect("resource trace phases lock poisoned")
                .clone(),
        }
    }
}

pub(crate) fn record_phase(
    operation: &str,
    args: &Value,
    duration: Duration,
    success: bool,
    error: Option<String>,
) {
    let _ = CURRENT_RESOURCE_TRACE.try_with(|trace| {
        let phase = QueryPhaseView {
            operation: operation.to_string(),
            started_at: crate::current_timestamp(),
            duration_ms: duration_to_ms(duration),
            args_summary: Some(summarize_value(args)),
            touched: touches_for_value(args),
            success,
            error,
        };
        trace
            .phases
            .lock()
            .expect("resource trace phases lock poisoned")
            .push(phase);
    });
}
