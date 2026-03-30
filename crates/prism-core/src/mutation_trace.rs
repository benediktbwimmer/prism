use serde_json::Value;
use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct MutationTracePhase {
    pub operation: String,
    pub args: Value,
    pub duration: Duration,
    pub success: bool,
    pub error: Option<String>,
}

thread_local! {
    static TRACE_STACK: RefCell<Vec<Arc<Mutex<Vec<MutationTracePhase>>>>> = const { RefCell::new(Vec::new()) };
}

pub fn scope<T>(operation: impl FnOnce() -> T) -> (T, Vec<MutationTracePhase>) {
    let sink = Arc::new(Mutex::new(Vec::new()));
    TRACE_STACK.with(|stack| stack.borrow_mut().push(Arc::clone(&sink)));
    let result = operation();
    TRACE_STACK.with(|stack| {
        stack.borrow_mut().pop();
    });
    let phases = sink.lock().expect("mutation trace sink poisoned").clone();
    (result, phases)
}

pub fn record_phase(
    operation: &str,
    args: Value,
    duration: Duration,
    success: bool,
    error: Option<String>,
) {
    TRACE_STACK.with(|stack| {
        let Some(sink) = stack.borrow().last().cloned() else {
            return;
        };
        sink.lock()
            .expect("mutation trace sink poisoned")
            .push(MutationTracePhase {
                operation: operation.to_string(),
                args,
                duration,
                success,
                error,
            });
    });
}
