use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{anyhow, Context as AnyhowContext, Result};
use deno_ast::{
    parse_program, EmitOptions, MediaType, ModuleSpecifier, ParseParams, TranspileModuleOptions,
    TranspileOptions,
};
use prism_js::runtime_prelude;
use rquickjs::{
    promise::MaybePromise, prelude::Func, CatchResultExt, CaughtError, Context, Runtime,
};
use serde_json::json;
use tracing::error;

use crate::logging::format_error_chain;
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
                error!(
                    error = %error,
                    error_chain = %format_error_chain(&error),
                    "prism-mcp js worker failed"
                );
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
            .catch(&ctx)
            .map_err(|error| {
                format_caught_js_error("failed to load prism runtime prelude", error)
            })?;
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
                    let promise = ctx
                        .eval::<MaybePromise<'_>, _>(request.script.as_str())
                        .catch(&ctx)
                        .map_err(|error| {
                            format_caught_js_error("javascript query evaluation failed", error)
                        })?;
                    promise
                        .finish::<String>()
                        .catch(&ctx)
                        .map_err(|error| {
                            format_caught_js_error("javascript query evaluation failed", error)
                        })
                });

                let cleanup_result = context.with(|ctx| -> Result<()> {
                    ctx.eval::<(), _>("__prismCleanupGlobals()")
                        .catch(&ctx)
                        .map_err(|error| format_caught_js_error("javascript cleanup failed", error))
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

fn format_caught_js_error(prefix: &str, error: CaughtError<'_>) -> anyhow::Error {
    match error {
        CaughtError::Exception(exception) => {
            let message = exception
                .message()
                .filter(|message| !message.is_empty())
                .unwrap_or_else(|| exception.to_string());
            let mut detail = format!("{prefix}: {message}");
            if let Some(stack) = exception.stack().filter(|stack| !stack.is_empty()) {
                if stack.contains(&message) {
                    detail.push('\n');
                    detail.push_str(&stack);
                } else {
                    detail.push_str("\nstack: ");
                    detail.push_str(&stack);
                }
            }
            anyhow!(detail)
        }
        CaughtError::Value(value) => anyhow!(format!(
            "{prefix}: javascript threw a non-Error value: {value:?}"
        )),
        CaughtError::Error(error) => anyhow!(format!("{prefix}: {error}")),
    }
}
