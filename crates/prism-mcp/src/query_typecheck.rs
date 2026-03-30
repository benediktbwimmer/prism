use std::collections::HashMap;

use anyhow::Result;
use deno_ast::swc::ast::{
    BlockStmt, CallExpr, Callee, Expr, ExprOrSpread, Lit, MemberExpr, MemberProp, ObjectLit,
    OptCall, Pat, Program, Prop, PropName, PropOrSpread, VarDeclarator,
};
use deno_ast::swc::common::Spanned;
use deno_ast::swc::ecma_visit::{Visit, VisitWith};
use deno_ast::{parse_program, MediaType, ModuleSpecifier, ParseParams, ParsedSource, SourcePos};
use prism_js::{
    prism_method_spec, prism_object_property_type, prism_surface_type_for_method, PrismSurfaceType,
};
use serde_json::{json, Map, Value};

use crate::{
    closest_prism_api_path, query_views::known_query_view_names, static_typecheck_error,
    suggest_api_token,
};

const ARRAY_BUILTINS: &[&str] = &[
    "length", "map", "filter", "find", "some", "every", "flatMap", "at", "slice", "join",
    "includes", "forEach",
];

#[derive(Clone, Copy)]
pub(crate) enum StaticCheckMode {
    StatementBody,
    ImplicitExpression,
}

impl StaticCheckMode {
    pub(crate) fn attempted_mode(self) -> &'static str {
        match self {
            Self::StatementBody => "statement_body",
            Self::ImplicitExpression => "implicit_expression",
        }
    }

    fn wrapped_source(self, code: &str) -> String {
        match self {
            Self::StatementBody => format!("async function __prismTypecheck() {{\n{code}\n}}\n"),
            Self::ImplicitExpression => format!("(\n{code}\n);\n"),
        }
    }
}

pub(crate) fn typecheck_query(code: &str, mode: StaticCheckMode) -> Result<()> {
    let wrapped = mode.wrapped_source(code);
    let parsed = match parse_program(ParseParams {
        specifier: ModuleSpecifier::parse("file:///prism/typecheck.ts")?,
        text: wrapped.into(),
        media_type: MediaType::TypeScript,
        capture_tokens: false,
        maybe_syntax: None,
        scope_analysis: false,
    }) {
        Ok(parsed) => parsed,
        Err(_) => return Ok(()),
    };
    let mut checker = PrismApiTypechecker {
        parsed: &parsed,
        attempted_mode: mode.attempted_mode(),
        issue: None,
        scopes: vec![HashMap::new()],
    };
    parsed.program_ref().visit_with(&mut checker);
    match checker.issue {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

struct PrismApiTypechecker<'a> {
    parsed: &'a ParsedSource,
    attempted_mode: &'static str,
    issue: Option<anyhow::Error>,
    scopes: Vec<HashMap<String, PrismSurfaceType>>,
}

impl PrismApiTypechecker<'_> {
    fn is_dynamic_query_view_method(&self, method_path: &str) -> bool {
        let Some(name) = method_path.strip_prefix("prism.") else {
            return false;
        };
        known_query_view_names()
            .into_iter()
            .any(|candidate| candidate == name)
    }

    fn push_issue(&mut self, error: anyhow::Error) {
        if self.issue.is_none() {
            self.issue = Some(error);
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    fn bind(&mut self, name: String, ty: PrismSurfaceType) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, ty);
        }
    }

    fn lookup(&self, name: &str) -> Option<PrismSurfaceType> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
    }

    fn line_column(&self, span_lo: deno_ast::swc::common::BytePos) -> (usize, usize) {
        let pos = SourcePos::unsafely_from_byte_pos(span_lo);
        let display = self.parsed.text_info_lazy().line_and_column_display(pos);
        (display.line_number.saturating_sub(1), display.column_number)
    }

    fn typecheck_error(
        &self,
        span_lo: deno_ast::swc::common::BytePos,
        detail: impl Into<String>,
        data: Value,
    ) -> anyhow::Error {
        let (line, column) = self.line_column(span_lo);
        static_typecheck_error(detail, self.attempted_mode, line, column, data)
    }

    fn unknown_method_error(
        &self,
        observed: &str,
        span_lo: deno_ast::swc::common::BytePos,
    ) -> anyhow::Error {
        let suggestion = closest_prism_api_path(observed);
        let next_action = suggestion.as_ref().map_or_else(
            || {
                "Check `prism://api-reference` for the stable PRISM query helpers, then retry."
                    .to_string()
            },
            |suggested| {
                format!(
                    "`{observed}` is not a stable PRISM query helper. Did you mean `{suggested}`? Check `prism://api-reference` for the exact method signature and retry."
                )
            },
        );
        self.typecheck_error(
            span_lo,
            format!("`{observed}` is not part of the stable PRISM query API."),
            json!({
                "observedPath": observed,
                "didYouMean": suggestion,
                "nextAction": next_action,
            }),
        )
    }

    fn invalid_record_error(
        &self,
        method_path: &str,
        argument_name: &str,
        span_lo: deno_ast::swc::common::BytePos,
        detail: String,
        next_action: String,
        extra_data: Map<String, Value>,
    ) -> anyhow::Error {
        let mut data = extra_data;
        data.insert("method".to_string(), json!(method_path));
        data.insert("argumentName".to_string(), json!(argument_name));
        data.insert("error".to_string(), json!(detail));
        data.insert("nextAction".to_string(), json!(next_action));
        self.typecheck_error(span_lo, detail, Value::Object(data))
    }

    fn invalid_property_error(
        &self,
        property: &str,
        span_lo: deno_ast::swc::common::BytePos,
        candidates: &[String],
    ) -> anyhow::Error {
        let candidate_refs = candidates.iter().map(String::as_str).collect::<Vec<_>>();
        let suggestion = suggest_api_token(property, &candidate_refs);
        let next_action = suggestion.as_ref().map_or_else(
            || {
                "Use a documented property on the returned PRISM view or inspect `prism://api-reference` for the result shape."
                    .to_string()
            },
            |suggested| {
                format!(
                    "Use the documented property `{suggested}` instead of `{property}` and retry. Check `prism://api-reference` for the result shape."
                )
            },
        );
        self.typecheck_error(
            span_lo,
            format!("unknown property `{property}` on a typed PRISM query result."),
            json!({
                "property": property,
                "didYouMean": suggestion,
                "nextAction": next_action,
            }),
        )
    }

    fn infer_expr(&mut self, expr: &Expr) -> PrismSurfaceType {
        if self.issue.is_some() {
            return PrismSurfaceType::Unknown;
        }
        match expr {
            Expr::Ident(ident) => self
                .lookup(ident.sym.as_ref())
                .unwrap_or(PrismSurfaceType::Unknown),
            Expr::Lit(Lit::Str(_)) => PrismSurfaceType::Primitive("string"),
            Expr::Lit(Lit::Num(_)) | Expr::Lit(Lit::BigInt(_)) => {
                PrismSurfaceType::Primitive("number")
            }
            Expr::Lit(Lit::Bool(_)) => PrismSurfaceType::Primitive("boolean"),
            Expr::Lit(Lit::Null(_)) => PrismSurfaceType::Unknown,
            Expr::Paren(paren) => self.infer_expr(&paren.expr),
            Expr::Await(await_expr) => self.infer_expr(&await_expr.arg),
            Expr::Object(object) => self.infer_object(object),
            Expr::Array(array) => {
                let item = array
                    .elems
                    .iter()
                    .flatten()
                    .map(|element| self.infer_expr(&element.expr))
                    .find(|ty| !matches!(ty, PrismSurfaceType::Unknown))
                    .unwrap_or(PrismSurfaceType::Unknown);
                PrismSurfaceType::Array(Box::new(item))
            }
            Expr::Call(call) => self.infer_call_expr(call),
            Expr::Member(member) => self.infer_member_expr(member),
            Expr::OptChain(chain) => match &*chain.base {
                deno_ast::swc::ast::OptChainBase::Member(member) => self.infer_member_expr(member),
                deno_ast::swc::ast::OptChainBase::Call(call) => self.infer_opt_call_expr(call),
            },
            _ => PrismSurfaceType::Unknown,
        }
    }

    fn infer_object(&mut self, object: &ObjectLit) -> PrismSurfaceType {
        let mut properties = HashMap::new();
        for prop in &object.props {
            let Some(name) = property_name(prop) else {
                continue;
            };
            let Some(value_expr) = property_value(prop) else {
                continue;
            };
            properties.insert(name, self.infer_expr(value_expr));
        }
        PrismSurfaceType::Object(properties.into_iter().collect())
    }

    fn infer_call_expr(&mut self, call: &CallExpr) -> PrismSurfaceType {
        let Some(method_path) = call_path(call) else {
            for arg in &call.args {
                self.infer_expr(&arg.expr);
            }
            return PrismSurfaceType::Unknown;
        };
        let Some(method) = prism_method_spec(&method_path) else {
            if self.is_dynamic_query_view_method(&method_path) {
                for arg in &call.args {
                    self.infer_expr(&arg.expr);
                }
                return PrismSurfaceType::Unknown;
            }
            self.push_issue(self.unknown_method_error(&method_path, call.span.lo));
            return PrismSurfaceType::Unknown;
        };
        if let Some(record) = method.record_arg {
            if method.path == "prism.decodeConcept"
                && call
                    .args
                    .first()
                    .is_some_and(|arg| matches!(&*arg.expr, Expr::Lit(Lit::Str(_))))
            {
                return prism_surface_type_for_method(method.path)
                    .unwrap_or(PrismSurfaceType::Unknown);
            }
            if let Some(argument) = call.args.get(record.arg_index) {
                self.validate_record_argument(
                    method.path,
                    record.arg_name,
                    argument,
                    record.allowed_keys,
                );
            }
        }
        prism_surface_type_for_method(method.path).unwrap_or(PrismSurfaceType::Unknown)
    }

    fn infer_opt_call_expr(&mut self, call: &OptCall) -> PrismSurfaceType {
        let Some(method_path) = expr_path(&call.callee) else {
            for arg in &call.args {
                self.infer_expr(&arg.expr);
            }
            return PrismSurfaceType::Unknown;
        };
        let Some(method) = prism_method_spec(&method_path) else {
            if self.is_dynamic_query_view_method(&method_path) {
                for arg in &call.args {
                    self.infer_expr(&arg.expr);
                }
                return PrismSurfaceType::Unknown;
            }
            self.push_issue(self.unknown_method_error(&method_path, call.span.lo));
            return PrismSurfaceType::Unknown;
        };
        if let Some(record) = method.record_arg {
            if method.path == "prism.decodeConcept"
                && call
                    .args
                    .first()
                    .is_some_and(|arg| matches!(&*arg.expr, Expr::Lit(Lit::Str(_))))
            {
                return prism_surface_type_for_method(method.path)
                    .unwrap_or(PrismSurfaceType::Unknown);
            }
            if let Some(argument) = call.args.get(record.arg_index) {
                self.validate_record_argument(
                    method.path,
                    record.arg_name,
                    argument,
                    record.allowed_keys,
                );
            }
        }
        prism_surface_type_for_method(method.path).unwrap_or(PrismSurfaceType::Unknown)
    }

    fn validate_record_argument(
        &mut self,
        method_path: &str,
        argument_name: &str,
        argument: &ExprOrSpread,
        allowed_keys: &[&str],
    ) {
        if self.issue.is_some() {
            return;
        }
        match &*argument.expr {
            Expr::Object(object) => {
                let mut invalid_keys = Vec::new();
                let mut did_you_mean = Map::new();
                for prop in &object.props {
                    let Some(name) = property_name(prop) else {
                        continue;
                    };
                    if allowed_keys.contains(&name.as_str()) {
                        continue;
                    }
                    if let Some(suggestion) = suggest_api_token(&name, allowed_keys) {
                        did_you_mean.insert(name.clone(), Value::String(suggestion));
                    }
                    invalid_keys.push(name);
                }
                if invalid_keys.is_empty() {
                    return;
                }
                let invalid_summary = invalid_keys
                    .iter()
                    .map(|key| format!("`{key}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let suggestion_summary = did_you_mean
                    .iter()
                    .map(|(key, value)| format!("`{key}` -> `{}`", value.as_str().unwrap_or("")))
                    .collect::<Vec<_>>()
                    .join(", ");
                let next_action = if suggestion_summary.is_empty() {
                    format!(
                        "Use only documented keys for `{method_path}` and retry. Check `prism://api-reference` for the exact shape."
                    )
                } else {
                    format!(
                        "Use the documented key spelling instead ({suggestion_summary}) and retry. Check `prism://api-reference` for the exact shape."
                    )
                };
                let detail = format!(
                    "unknown {} {invalid_summary} in `{argument_name}` for `{method_path}`.",
                    if invalid_keys.len() == 1 {
                        "key"
                    } else {
                        "keys"
                    },
                );
                self.push_issue(self.invalid_record_error(
                    method_path,
                    argument_name,
                    argument.expr.span().lo,
                    detail,
                    next_action,
                    Map::from_iter([
                        ("invalidKeys".to_string(), json!(invalid_keys)),
                        ("didYouMean".to_string(), Value::Object(did_you_mean)),
                    ]),
                ));
            }
            Expr::Lit(Lit::Null(_)) | Expr::Ident(_) | Expr::Member(_) | Expr::Call(_) => {}
            Expr::Paren(paren) => self.validate_record_argument(
                method_path,
                argument_name,
                &ExprOrSpread {
                    spread: None,
                    expr: paren.expr.clone(),
                },
                allowed_keys,
            ),
            _ => {
                let next_action = format!(
                    "Pass a plain object for `{argument_name}` on `{method_path}` or omit it entirely. Check `prism://api-reference` for the exact shape."
                );
                self.push_issue(self.invalid_record_error(
                    method_path,
                    argument_name,
                    argument.expr.span().lo,
                    "record-shaped arguments must be plain objects when provided.".to_string(),
                    next_action,
                    Map::new(),
                ));
            }
        }
    }

    fn infer_member_expr(&mut self, member: &MemberExpr) -> PrismSurfaceType {
        if expr_path_from_member(member).is_some_and(|path| path.starts_with("prism.")) {
            return PrismSurfaceType::Unknown;
        }
        let object_type = self.infer_expr(&member.obj);
        match &member.prop {
            MemberProp::Computed(computed) => {
                if let Expr::Lit(Lit::Num(_)) = &*computed.expr {
                    if let PrismSurfaceType::Array(item) = unwrap_nullable(&object_type) {
                        return item.as_ref().clone();
                    }
                    return PrismSurfaceType::Unknown;
                }
                if let Expr::Lit(Lit::Str(value)) = &*computed.expr {
                    return self.lookup_property(
                        &object_type,
                        &value.value.to_string_lossy(),
                        computed.expr.span().lo,
                    );
                }
                PrismSurfaceType::Unknown
            }
            MemberProp::Ident(ident) => {
                let name = ident.sym.to_string();
                if matches!(unwrap_nullable(&object_type), PrismSurfaceType::Array(_))
                    && ARRAY_BUILTINS.contains(&name.as_str())
                {
                    return PrismSurfaceType::Unknown;
                }
                self.lookup_property(&object_type, &name, ident.span.lo)
            }
            MemberProp::PrivateName(_) => PrismSurfaceType::Unknown,
        }
    }

    fn lookup_property(
        &mut self,
        object_type: &PrismSurfaceType,
        property: &str,
        span_lo: deno_ast::swc::common::BytePos,
    ) -> PrismSurfaceType {
        if let Some(value) = prism_object_property_type(object_type, property) {
            return value;
        }
        let candidates = match unwrap_nullable(object_type) {
            PrismSurfaceType::Object(properties) => properties.keys().cloned().collect::<Vec<_>>(),
            _ => return PrismSurfaceType::Unknown,
        };
        self.push_issue(self.invalid_property_error(property, span_lo, &candidates));
        PrismSurfaceType::Unknown
    }
}

impl Visit for PrismApiTypechecker<'_> {
    fn visit_program(&mut self, program: &Program) {
        if self.issue.is_none() {
            program.visit_children_with(self);
        }
    }

    fn visit_block_stmt(&mut self, block: &BlockStmt) {
        self.push_scope();
        if self.issue.is_none() {
            block.visit_children_with(self);
        }
        self.pop_scope();
    }

    fn visit_var_declarator(&mut self, declarator: &VarDeclarator) {
        if self.issue.is_some() {
            return;
        }
        if let (Pat::Ident(ident), Some(init)) = (&declarator.name, &declarator.init) {
            let ty = self.infer_expr(init);
            self.bind(ident.id.sym.to_string(), ty);
        } else if let Some(init) = &declarator.init {
            self.infer_expr(init);
        }
    }

    fn visit_expr(&mut self, expr: &Expr) {
        if self.issue.is_none() {
            self.infer_expr(expr);
        }
    }
}

fn unwrap_nullable(surface: &PrismSurfaceType) -> &PrismSurfaceType {
    match surface {
        PrismSurfaceType::Nullable(inner) => unwrap_nullable(inner),
        other => other,
    }
}

fn property_name(prop: &PropOrSpread) -> Option<String> {
    match prop {
        PropOrSpread::Spread(_) => None,
        PropOrSpread::Prop(prop) => match &**prop {
            Prop::KeyValue(key_value) => prop_name(&key_value.key),
            Prop::Shorthand(ident) => Some(ident.sym.to_string()),
            Prop::Assign(assign) => Some(assign.key.sym.to_string()),
            Prop::Getter(getter) => prop_name(&getter.key),
            Prop::Setter(setter) => prop_name(&setter.key),
            Prop::Method(method) => prop_name(&method.key),
        },
    }
}

fn property_value(prop: &PropOrSpread) -> Option<&Expr> {
    match prop {
        PropOrSpread::Spread(_) => None,
        PropOrSpread::Prop(prop) => match &**prop {
            Prop::KeyValue(key_value) => Some(&key_value.value),
            Prop::Shorthand(_) => None,
            Prop::Assign(assign) => Some(&assign.value),
            Prop::Getter(_) | Prop::Setter(_) | Prop::Method(_) => None,
        },
    }
}

fn prop_name(name: &PropName) -> Option<String> {
    match name {
        PropName::Ident(ident) => Some(ident.sym.to_string()),
        PropName::Str(value) => Some(value.value.to_string_lossy().to_string()),
        PropName::Num(value) => Some(value.value.to_string()),
        PropName::BigInt(value) => Some(value.value.to_string()),
        PropName::Computed(_) => None,
    }
}

fn call_path(call: &CallExpr) -> Option<String> {
    let Callee::Expr(expr) = &call.callee else {
        return None;
    };
    expr_path(expr)
}

fn expr_path(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(ident) if ident.sym == "prism" => Some("prism".to_string()),
        Expr::Member(member) => expr_path_from_member(member),
        _ => None,
    }
}

fn expr_path_from_member(member: &MemberExpr) -> Option<String> {
    let object_path = match &*member.obj {
        Expr::Call(call) => {
            let base = call_path(call)?;
            (base == "prism.file").then(|| "prism.file(path)".to_string())
        }
        other => expr_path(other),
    }?;
    let prop = match &member.prop {
        MemberProp::Ident(ident) => ident.sym.to_string(),
        MemberProp::PrivateName(_) | MemberProp::Computed(_) => return None,
    };
    Some(format!("{object_path}.{prop}"))
}
