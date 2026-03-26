use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{anyhow, Context as AnyhowContext, Result};
use deno_ast::{
    parse_program, EmitOptions, MediaType, ModuleSpecifier, ParseParams, TranspileModuleOptions,
    TranspileOptions,
};
use prism_js::runtime_prelude;
use rquickjs::{prelude::Func, Context, Runtime};
use serde_json::json;

use crate::QueryExecution;

pub(crate) struct JsWorker {
    tx: mpsc::Sender<JsWorkerMessage>,
}

struct JsWorkerRequest {
    script: String,
    execution: QueryExecution,
    reply: mpsc::Sender<Result<String>>,
}

enum JsWorkerMessage {
    Execute(JsWorkerRequest),
}

impl JsWorker {
    pub(crate) fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<JsWorkerMessage>();
        thread::spawn(move || {
            if let Err(error) = run_js_worker(rx) {
                eprintln!("prism-mcp js worker failed: {error}");
            }
        });
        Self { tx }
    }

    pub(crate) fn execute(&self, script: String, execution: QueryExecution) -> Result<String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(JsWorkerMessage::Execute(JsWorkerRequest {
                script,
                execution,
                reply: reply_tx,
            }))
            .map_err(|_| anyhow!("js worker is unavailable"))?;

        reply_rx
            .recv()
            .map_err(|_| anyhow!("js worker dropped the query response"))?
    }
}

fn run_js_worker(rx: mpsc::Receiver<JsWorkerMessage>) -> Result<()> {
    let runtime = Runtime::new().context("failed to create JS runtime")?;
    let context = Context::full(&runtime).context("failed to create JS context")?;
    let active_execution = Arc::new(Mutex::new(None::<QueryExecution>));

    context.with(|ctx| -> Result<()> {
        let current = active_execution.clone();
        ctx.globals().set(
            "__prismHostCall",
            Func::from(move |operation: String, args_json: String| {
                let execution = {
                    let guard = current.lock().expect("active execution lock poisoned");
                    guard.clone()
                };
                let Some(execution) = execution else {
                    return json!({
                        "ok": false,
                        "error": "no active prism query execution"
                    })
                    .to_string();
                };
                execution.dispatch_enveloped(&operation, &args_json)
            }),
        )?;
        ctx.eval::<(), _>(runtime_prelude())
            .map_err(|err| anyhow!(err.to_string()))?;
        Ok(())
    })?;

    while let Ok(message) = rx.recv() {
        match message {
            JsWorkerMessage::Execute(request) => {
                {
                    let mut guard = active_execution
                        .lock()
                        .expect("active execution lock poisoned");
                    *guard = Some(request.execution.clone());
                }

                let result = context.with(|ctx| -> Result<String> {
                    ctx.eval::<String, _>(request.script.as_str())
                        .map_err(|err| anyhow!(err.to_string()))
                });

                let cleanup_result = context.with(|ctx| -> Result<()> {
                    ctx.eval::<(), _>("__prismCleanupGlobals()")
                        .map_err(|err| anyhow!(err.to_string()))
                });

                {
                    let mut guard = active_execution
                        .lock()
                        .expect("active execution lock poisoned");
                    *guard = None;
                }

                let final_result = match (result, cleanup_result) {
                    (Ok(value), Ok(())) => Ok(value),
                    (Err(error), _) => Err(error),
                    (Ok(_), Err(error)) => Err(error),
                };

                let _ = request.reply.send(final_result);
            }
        }
    }

    Ok(())
}

pub(crate) fn transpile_typescript(source: &str) -> Result<String> {
    let specifier = ModuleSpecifier::parse("file:///prism/query.ts")?;
    let parsed = parse_program(ParseParams {
        specifier,
        text: source.into(),
        media_type: MediaType::TypeScript,
        capture_tokens: false,
        maybe_syntax: None,
        scope_analysis: false,
    })
    .map_err(|err| anyhow!(err.to_string()))?;
    let transpiled = parsed
        .transpile(
            &TranspileOptions::default(),
            &TranspileModuleOptions::default(),
            &EmitOptions::default(),
        )
        .map_err(|err| anyhow!(err.to_string()))?
        .into_source();
    Ok(transpiled.text)
}
