use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use crate::query_surface::prism_api_method_specs;

fn js_string(value: &str) -> String {
    serde_json::to_string(value).expect("surface registry string should serialize")
}

pub fn runtime_option_keys_js_object() -> &'static str {
    static JS: OnceLock<String> = OnceLock::new();
    JS.get_or_init(|| {
        let mut seen = BTreeSet::new();
        let mut entries = Vec::new();
        for bundle in prism_api_method_specs()
            .iter()
            .filter_map(|spec| spec.record_arg)
        {
            if !seen.insert(bundle.bundle_name) {
                continue;
            }
            let keys = bundle
                .allowed_keys
                .iter()
                .map(|key| format!("\"{key}\""))
                .collect::<Vec<_>>()
                .join(", ");
            entries.push(format!(
                "  {}: Object.freeze([{}])",
                bundle.bundle_name, keys
            ));
        }
        format!("Object.freeze({{\n{}\n}})", entries.join(",\n"))
    })
    .as_str()
}

fn compiler_return_wrapper(path: &str) -> Option<&'static str> {
    match path {
        "prism.claim.acquire" | "prism.claim.renew" | "prism.claim.release" => Some("claim"),
        "prism.artifact.propose" | "prism.artifact.supersede" | "prism.artifact.review" => {
            Some("artifact")
        }
        "prism.coordination.createPlan" | "prism.coordination.openPlan" => Some("plan"),
        "prism.coordination.openTask"
        | "plan.addTask"
        | "task.update"
        | "task.complete"
        | "task.handoff"
        | "task.acceptHandoff"
        | "task.resume"
        | "task.reclaim" => Some("task"),
        _ => None,
    }
}

fn compiler_root_family(path: &str) -> Option<&'static str> {
    match path {
        path if path.starts_with("prism.work.") => Some("work"),
        path if path.starts_with("prism.claim.") => Some("claim"),
        path if path.starts_with("prism.artifact.") => Some("artifact"),
        path if path.starts_with("prism.coordination.") => Some("coordination"),
        _ => None,
    }
}

fn compiler_method_name(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or(path)
}

fn compiler_root_method_entries_js() -> String {
    let mut families: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for spec in prism_api_method_specs()
        .iter()
        .filter(|spec| spec.compiler.is_some())
    {
        if let Some(family) = compiler_root_family(spec.path) {
            families.entry(family).or_default().push(spec.path);
        }
    }
    let mut family_entries = Vec::new();
    for (family, methods) in families {
        let method_entries = methods
            .into_iter()
            .map(|path| {
                format!(
                    "    {}(...args) {{ return __prismCompilerInvoke({}, null, args); }}",
                    compiler_method_name(path),
                    js_string(path)
                )
            })
            .collect::<Vec<_>>()
            .join(",\n");
        family_entries.push(format!(
            "  {}: Object.freeze({{\n{}\n  }})",
            family, method_entries
        ));
    }
    format!("Object.freeze({{\n{}\n}})", family_entries.join(",\n"))
}

fn compiler_handle_method_sets_js() -> String {
    const CLAIM_METHODS: &[&str] = &["prism.claim.renew", "prism.claim.release"];
    const ARTIFACT_METHODS: &[&str] = &["prism.artifact.supersede", "prism.artifact.review"];
    const PLAN_METHODS: &[&str] = &["plan.update", "plan.archive", "plan.addTask"];
    const TASK_METHODS: &[&str] = &[
        "task.update",
        "task.complete",
        "task.handoff",
        "task.acceptHandoff",
        "task.resume",
        "task.reclaim",
        "task.dependsOn",
    ];
    let handle_sets = [
        ("claim", CLAIM_METHODS),
        ("artifact", ARTIFACT_METHODS),
        ("plan", PLAN_METHODS),
        ("task", TASK_METHODS),
    ];
    let entries = handle_sets
        .iter()
        .map(|(kind, methods)| {
            let values = methods
                .iter()
                .map(|path| js_string(path))
                .collect::<Vec<_>>()
                .join(", ");
            format!("  {}: Object.freeze([{}])", kind, values)
        })
        .collect::<Vec<_>>()
        .join(",\n");
    format!("Object.freeze({{\n{}\n}})", entries)
}

fn compiler_method_registry_js() -> String {
    let entries = prism_api_method_specs()
        .iter()
        .filter_map(|spec| spec.compiler.map(|compiler| (*spec, compiler)))
        .map(|(spec, compiler)| {
            let record_bundle = spec
                .record_arg
                .map(|bundle| js_string(bundle.bundle_name))
                .unwrap_or_else(|| "null".to_string());
            let arg_name = spec
                .record_arg
                .map(|bundle| js_string(bundle.arg_name))
                .unwrap_or_else(|| "null".to_string());
            let arg_index = spec
                .record_arg
                .map(|bundle| bundle.arg_index.to_string())
                .unwrap_or_else(|| "null".to_string());
            let host_operation = compiler
                .host_operation
                .map(js_string)
                .unwrap_or_else(|| "null".to_string());
            let wrapper = compiler_return_wrapper(spec.path)
                .map(js_string)
                .unwrap_or_else(|| "null".to_string());
            format!(
                "  {}: Object.freeze({{ methodName: {}, hostOperation: {}, recordBundle: {}, argName: {}, argIndex: {}, wrapper: {} }})",
                js_string(spec.path),
                js_string(compiler_method_name(spec.path)),
                host_operation,
                record_bundle,
                arg_name,
                arg_index,
                wrapper
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    format!("Object.freeze({{\n{}\n}})", entries)
}

pub fn compiler_runtime_js_prelude() -> &'static str {
    static JS: OnceLock<String> = OnceLock::new();
    JS.get_or_init(|| {
        format!(
            r#"const __prismCompilerMethodRegistry = {};
const __prismCompilerHandleMethods = {};
const __prismCompilerRootFamilies = {};

function __prismWrapCompilerHandle(raw, handleKind) {{
  if (raw == null || typeof raw !== "object") {{
    return raw;
  }}
  const methodPaths = __prismCompilerHandleMethods[handleKind];
  if (!Array.isArray(methodPaths) || methodPaths.length === 0) {{
    return raw;
  }}
  const wrapped = {{ ...raw }};
  for (const methodPath of methodPaths) {{
    const spec = __prismCompilerMethodRegistry[methodPath];
    if (!spec || typeof spec.methodName !== "string") {{
      continue;
    }}
    wrapped[spec.methodName] = (...args) => __prismCompilerInvoke(methodPath, raw, args);
  }}
  if (handleKind === "plan" && typeof wrapped.task !== "function" && typeof wrapped.addTask === "function") {{
    wrapped.task = (input = {{}}) => wrapped.addTask(input);
  }}
  return wrapped;
}}

function __prismWrapCompilerResult(methodPath, raw) {{
  const spec = __prismCompilerMethodRegistry[methodPath];
  const wrapper = spec?.wrapper;
  switch (wrapper) {{
    case "plan":
    case "task":
    case "claim":
    case "artifact":
      return __prismWrapCompilerHandle(raw, wrapper);
    default:
      return raw;
  }}
}}

function __prismCompilerValidateInput(methodPath, spec, args) {{
  if (!spec || spec.recordBundle == null) {{
    return null;
  }}
  const input = args?.[spec.argIndex] ?? {{}};
  return __prismValidateRecordShape(
    methodPath,
    input,
    spec.argName ?? "input",
    __prismOptionKeys[spec.recordBundle]
  );
}}

function __prismCompilerInvoke(methodPath, receiver, args = []) {{
  const spec = __prismCompilerMethodRegistry[methodPath];
  if (!spec || typeof spec.hostOperation !== "string") {{
    __prismThrowQueryUserError(
      "prism_code compiler method unavailable",
      `No compiler runtime metadata exists for \`${{methodPath}}\`.`,
      {{
        code: "compiler_method_missing",
        category: "compiler_runtime",
        method: methodPath,
      }}
    );
  }}
  switch (methodPath) {{
    case "prism.work.declare": {{
      const input = __prismCompilerValidateInput(methodPath, spec, args);
      __prismRequiredStringField(methodPath, input, "title");
      return __prismWrapCompilerResult(methodPath, __prismHost(spec.hostOperation, {{ input }}));
    }}
    case "prism.claim.acquire": {{
      const input = __prismCompilerValidateInput(methodPath, spec, args);
      const coordinationTaskId = input?.coordinationTaskId ?? input?.coordination_task_id;
      return __prismWrapCompilerResult(methodPath, __prismHost(spec.hostOperation, {{
        input: {{
          anchors: input?.anchors,
          capability: input?.capability,
          mode: input?.mode,
          ttlSeconds: input?.ttlSeconds ?? input?.ttl_seconds,
          agent: input?.agent,
          coordinationTaskId:
            coordinationTaskId == null
              ? undefined
              : __prismTaskRef(coordinationTaskId, methodPath, "`coordinationTaskId`"),
        }},
      }}));
    }}
    case "prism.claim.renew": {{
      const claim = receiver ?? args[0];
      const input = __prismCompilerValidateInput(methodPath, spec, receiver == null ? args : [null, args[0]]);
      return __prismWrapCompilerResult(methodPath, __prismHost(spec.hostOperation, {{
        claim,
        input: {{
          ttlSeconds: input?.ttlSeconds ?? input?.ttl_seconds,
        }},
      }}));
    }}
    case "prism.claim.release": {{
      const claim = receiver ?? args[0];
      return __prismWrapCompilerResult(methodPath, __prismHost(spec.hostOperation, {{ claim }}));
    }}
    case "prism.artifact.propose": {{
      const input = __prismCompilerValidateInput(methodPath, spec, args);
      const taskId = input?.taskId ?? input?.task_id;
      return __prismWrapCompilerResult(methodPath, __prismHost(spec.hostOperation, {{
        input: {{
          taskId: __prismTaskRef(taskId, methodPath, "`taskId`"),
          artifactRequirementId:
            input?.artifactRequirementId ?? input?.artifact_requirement_id,
          anchors: input?.anchors,
          diffRef: input?.diffRef ?? input?.diff_ref,
          evidence: input?.evidence,
          requiredValidations:
            input?.requiredValidations ?? input?.required_validations,
          validatedChecks:
            input?.validatedChecks ?? input?.validated_checks,
          riskScore: input?.riskScore ?? input?.risk_score,
        }},
      }}));
    }}
    case "prism.artifact.supersede": {{
      const artifact = receiver ?? args[0];
      return __prismWrapCompilerResult(methodPath, __prismHost(spec.hostOperation, {{ artifact }}));
    }}
    case "prism.artifact.review": {{
      const artifact = receiver ?? args[0];
      const input = __prismCompilerValidateInput(methodPath, spec, receiver == null ? args : [null, args[0]]);
      return __prismWrapCompilerResult(methodPath, __prismHost(spec.hostOperation, {{
        artifact,
        input: {{
          reviewRequirementId:
            input?.reviewRequirementId ?? input?.review_requirement_id,
          verdict: input?.verdict,
          summary: input?.summary,
          requiredValidations:
            input?.requiredValidations ?? input?.required_validations,
          validatedChecks:
            input?.validatedChecks ?? input?.validated_checks,
          riskScore: input?.riskScore ?? input?.risk_score,
        }},
      }}));
    }}
    case "prism.coordination.createPlan": {{
      const input = __prismCompilerValidateInput(methodPath, spec, args);
      return __prismWrapCompilerResult(methodPath, __prismHost(spec.hostOperation, {{ input }}));
    }}
    case "prism.coordination.openPlan": {{
      const planId = args[0];
      if (typeof planId !== "string" || planId.trim() === "") {{
        __prismThrowQueryUserError(
          "prism_code coordination plan id invalid",
          "prism.coordination.openPlan(planId) requires a non-empty plan id string.",
          {{
            code: "coordination_plan_id_required",
            category: "coordination_builder",
            method: methodPath,
          }}
        );
      }}
      return __prismWrapCompilerResult(methodPath, __prismHost(spec.hostOperation, {{ planId: planId.trim() }}));
    }}
    case "prism.coordination.openTask": {{
      const taskId = args[0];
      if (typeof taskId !== "string" || taskId.trim() === "") {{
        __prismThrowQueryUserError(
          "prism_code coordination task id invalid",
          "prism.coordination.openTask(taskId) requires a non-empty task id string.",
          {{
            code: "coordination_task_id_required",
            category: "coordination_builder",
            method: methodPath,
          }}
        );
      }}
      return __prismWrapCompilerResult(methodPath, __prismHost(spec.hostOperation, {{ taskId: taskId.trim() }}));
    }}
    case "plan.update": {{
      const input = __prismCompilerValidateInput(methodPath, spec, args);
      return __prismWrapCompilerResult(methodPath, __prismHost(spec.hostOperation, {{
        plan: receiver,
        input,
      }}));
    }}
    case "plan.archive": {{
      return __prismWrapCompilerResult(methodPath, __prismHost(spec.hostOperation, {{ plan: receiver }}));
    }}
    case "plan.addTask": {{
      const input = __prismCompilerValidateInput(methodPath, spec, args);
      return __prismWrapCompilerResult(methodPath, __prismHost(spec.hostOperation, {{
        planHandleId: __prismCoordinationHandleId(receiver, methodPath, "plan"),
        input,
      }}));
    }}
    case "task.update":
    case "task.complete":
    case "task.handoff":
    case "task.acceptHandoff":
    case "task.resume":
    case "task.reclaim": {{
      const input = __prismCompilerValidateInput(methodPath, spec, args);
      return __prismWrapCompilerResult(methodPath, __prismHost(spec.hostOperation, {{
        task: receiver,
        input,
      }}));
    }}
    case "task.dependsOn": {{
      return __prismHost(spec.hostOperation, {{
        task: receiver,
        dependsOn: args[0],
        kind: args[1]?.kind,
      }});
    }}
    default:
      __prismThrowQueryUserError(
        "prism_code compiler method unavailable",
        `No compiler runtime invocation exists for \`${{methodPath}}\`.`,
        {{
          code: "compiler_method_missing",
          category: "compiler_runtime",
          method: methodPath,
        }}
      );
  }}
}}
"#,
            compiler_method_registry_js(),
            compiler_handle_method_sets_js(),
            compiler_root_method_entries_js(),
        )
    })
    .as_str()
}
