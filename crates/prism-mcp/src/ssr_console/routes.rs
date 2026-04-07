use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use axum::body::Body;
use axum::extract::{Form, Path, Query, State};
use axum::http::{
    header::{CONTENT_DISPOSITION, CONTENT_TYPE},
    HeaderMap, HeaderValue, Response, StatusCode,
};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::Router;
use prism_core::render_repo_published_plan_markdown;
use prism_ir::PlanId;
use serde::Deserialize;
use serde_json::json;

use super::assets::{console_css, console_js};
use super::concepts::{
    build_concept_slice, concept_handle_to_slug, concept_slug_to_handle, ConceptDirection,
};
use super::html::{
    duration_label, escape_html, json_script_escape, markdown_to_html, page_shell, percent,
    status_badge, status_slug, truncate,
};
use super::mermaid::concept_graph_mermaid;
use crate::ui_assets::prism_ui_favicon_asset;
use crate::ui_mutations::{map_ui_mutation_error, resolve_ui_mutation_args, PrismUiMutateRequest};
use crate::ui_read_models::{QueryHostUiReadModelsExt, UiPlansQueryOptions};
use crate::ui_router::PrismUiState;
use crate::ui_types::{PrismPlanDetailView, PrismPlansView, PrismUiFleetView};
use crate::{PrismMcpServer, QueryHost};

#[derive(Clone)]
pub(crate) struct PrismConsoleState {
    pub(crate) server: Arc<PrismMcpServer>,
    pub(crate) host: Arc<QueryHost>,
    pub(crate) root: PathBuf,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlansQuery {
    plan_id: Option<String>,
    status: Option<String>,
    search: Option<String>,
    sort: Option<String>,
    agent: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConceptQuery {
    handle: Option<String>,
    depth: Option<usize>,
    direction: Option<String>,
    relation: Option<String>,
    search: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct TaskEditForm {
    title: String,
    description: Option<String>,
    priority: Option<String>,
    assignee: Option<String>,
    status: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ClaimReleaseForm {
    claim_id: String,
}

pub(crate) fn routes(state: PrismConsoleState) -> Router {
    Router::new()
        .route("/console", get(console_overview))
        .route("/console/plans", get(console_plans_page))
        .route("/console/plans/{plan_id}", get(console_plan_page))
        .route(
            "/console/plans/{plan_id}/markdown",
            get(console_plan_markdown),
        )
        .route(
            "/console/plans/{plan_id}/archive",
            post(console_plan_archive),
        )
        .route("/console/fragments/plans", get(console_plans_fragment))
        .route(
            "/console/fragments/plans/{plan_id}",
            get(console_plan_fragment),
        )
        .route("/console/tasks/{task_id}", get(console_task_page))
        .route("/console/tasks/{task_id}/edit", post(console_task_edit))
        .route(
            "/console/tasks/{task_id}/release-lease",
            post(console_task_release_lease),
        )
        .route(
            "/console/fragments/tasks/{task_id}",
            get(console_task_fragment),
        )
        .route("/console/concepts", get(console_concepts_index))
        .route("/console/concepts/{slug}", get(console_concept_page))
        .route("/console/fleet", get(console_fleet_page))
        .route("/console/fragments/fleet", get(console_fleet_fragment))
        .route("/console/assets/console.css", get(console_css_asset))
        .route("/console/assets/console.js", get(console_js_asset))
        .route("/console/favicon.svg", get(console_favicon))
        .with_state(state)
}

async fn console_css_asset() -> impl IntoResponse {
    (
        [(
            CONTENT_TYPE,
            HeaderValue::from_static("text/css; charset=utf-8"),
        )],
        console_css(),
    )
}

async fn console_js_asset() -> impl IntoResponse {
    (
        [(
            CONTENT_TYPE,
            HeaderValue::from_static("text/javascript; charset=utf-8"),
        )],
        console_js(),
    )
}

async fn console_favicon(
    State(state): State<PrismConsoleState>,
) -> std::result::Result<Response<Body>, (StatusCode, String)> {
    let ui_state = PrismUiState {
        server: Arc::clone(&state.server),
        host: Arc::clone(&state.host),
        root: state.root.clone(),
    };
    prism_ui_favicon(State(ui_state)).await
}

async fn console_overview(
    State(state): State<PrismConsoleState>,
) -> std::result::Result<Html<String>, (StatusCode, String)> {
    let session = state
        .host
        .ui_session_bootstrap_view()
        .map_err(internal_error)?;
    let plans = state
        .host
        .ui_plans_view(UiPlansQueryOptions::default())
        .map_err(internal_error)?;
    let fleet = state.host.ui_fleet_view().map_err(internal_error)?;

    let active_bars = fleet.bars.iter().filter(|bar| bar.active).count();
    let blocked_plans = plans
        .plans
        .iter()
        .filter(|plan| format!("{:?}", plan.status) == "Blocked")
        .count();
    let body = format!(
        "<main class=\"console-layout console-layout--single\">\
         <section class=\"console-kpi-grid\">\
         <article class=\"console-kpi\"><span class=\"console-eyebrow\">Plans</span><strong>{}</strong><span class=\"console-muted console-small\">{} active · {} visible</span></article>\
         <article class=\"console-kpi\"><span class=\"console-eyebrow\">Fleet</span><strong>{}</strong><span class=\"console-muted console-small\">{} active leases</span></article>\
         <article class=\"console-kpi\"><span class=\"console-eyebrow\">Blocked</span><strong>{}</strong><span class=\"console-muted console-small\">plans needing intervention</span></article>\
         </section>\
         <section class=\"console-grid-two\">\
         <article class=\"console-card\"><div class=\"console-card-header\"><h2>Strategic focus</h2><a href=\"/console/plans\">Open plans</a></div>{}</article>\
         <article class=\"console-card\"><div class=\"console-card-header\"><h2>Fleet focus</h2><a href=\"/console/fleet\">Open fleet</a></div>{}</article>\
         </section>\
         </main>",
        plans.stats.total_plans,
        plans.stats.active_plans,
        plans.stats.visible_plans,
        fleet.lanes.len(),
        active_bars,
        blocked_plans,
        render_overview_plan_list(&plans),
        render_overview_fleet_list(&fleet),
    );
    Ok(Html(page_shell(
        "PRISM SSR Console",
        "A document-shaped operator surface for plans, concepts, and runtime coordination.",
        "overview",
        &session.session,
        &body,
    )))
}

async fn console_plans_page(
    State(state): State<PrismConsoleState>,
    Query(query): Query<PlansQuery>,
) -> std::result::Result<Html<String>, (StatusCode, String)> {
    let session = state
        .host
        .ui_session_bootstrap_view()
        .map_err(internal_error)?;
    let body = format!(
        "<main class=\"console-layout console-layout--single\">\
         <section id=\"console-plans-workspace\" class=\"console-live\" hx-get=\"{}\" hx-trigger=\"load, every 2s\" hx-swap=\"outerHTML\">\
         {}\
         </section></main>",
        plans_fragment_url(&query, None),
        render_plans_workspace(&state.host, query).map_err(internal_error)?
    );
    Ok(Html(page_shell(
        "SSR Plans",
        "Portfolio view with server-rendered dependency graphs and operator actions.",
        "plans",
        &session.session,
        &body,
    )))
}

async fn console_plan_page(
    State(state): State<PrismConsoleState>,
    Path(plan_id): Path<String>,
    Query(mut query): Query<PlansQuery>,
) -> std::result::Result<Html<String>, (StatusCode, String)> {
    query.plan_id = Some(plan_id);
    console_plans_page(State(state), Query(query)).await
}

async fn console_plans_fragment(
    State(state): State<PrismConsoleState>,
    Query(query): Query<PlansQuery>,
) -> std::result::Result<Html<String>, (StatusCode, String)> {
    render_plans_workspace(&state.host, query)
        .map(Html)
        .map_err(internal_error)
}

async fn console_plan_fragment(
    State(state): State<PrismConsoleState>,
    Path(plan_id): Path<String>,
) -> std::result::Result<Html<String>, (StatusCode, String)> {
    let view = state
        .host
        .ui_plan_detail_view(&plan_id)
        .map_err(internal_error)?
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("plan not found: {plan_id}")))?;
    Ok(Html(render_plan_detail(&view)))
}

async fn console_plan_markdown(
    State(state): State<PrismConsoleState>,
    Path(plan_id): Path<String>,
) -> std::result::Result<Response<Body>, (StatusCode, String)> {
    let Some((title, markdown)) =
        plan_markdown_payload(&state.host, &plan_id).map_err(internal_error)?
    else {
        return Err((StatusCode::NOT_FOUND, format!("plan not found: {plan_id}")));
    };
    let filename = format!(
        "{}.md",
        sanitize_download_basename(&format!("{title}-{plan_id}"))
    );
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/markdown; charset=utf-8")
        .header(
            CONTENT_DISPOSITION,
            HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
                .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?,
        )
        .body(Body::from(markdown))
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

async fn console_task_page(
    State(state): State<PrismConsoleState>,
    Path(task_id): Path<String>,
) -> std::result::Result<Html<String>, (StatusCode, String)> {
    let session = state
        .host
        .ui_session_bootstrap_view()
        .map_err(internal_error)?;
    let body = format!(
        "<main class=\"console-layout console-layout--single\">\
         <section id=\"console-task-detail\" class=\"console-live\" hx-get=\"/console/fragments/tasks/{}\" hx-trigger=\"load, every 2s\" hx-swap=\"outerHTML\">\
         {}\
         </section></main>",
        escape_html(&task_id),
        render_task_detail_fragment(&state.host, &task_id).map_err(internal_error)?
    );
    Ok(Html(page_shell(
        "SSR Task Detail",
        "Writable task detail with htmx-backed pessimistic coordination mutations.",
        "plans",
        &session.session,
        &body,
    )))
}

async fn console_task_fragment(
    State(state): State<PrismConsoleState>,
    Path(task_id): Path<String>,
) -> std::result::Result<Html<String>, (StatusCode, String)> {
    render_task_detail_fragment(&state.host, &task_id)
        .map(Html)
        .map_err(internal_error)
}

async fn console_task_edit(
    State(state): State<PrismConsoleState>,
    Path(task_id): Path<String>,
    Form(form): Form<TaskEditForm>,
) -> std::result::Result<impl IntoResponse, (StatusCode, String)> {
    let request = PrismUiMutateRequest {
        action: "coordination".to_string(),
        input: json!({
            "kind": "update",
            "payload": {
                "id": task_id,
                "title": form.title,
                "summary": sparse_patch(form.description),
                "assignee": sparse_patch(form.assignee),
                "priority": sparse_u8_patch(form.priority.as_deref()),
                "status": form.status,
            }
        }),
    };
    let args = resolve_ui_mutation_args(&state.root, state.host.workspace_session_ref(), request)
        .map_err(ui_error_to_status_message)?;
    let result = state
        .server
        .execute_prism_mutation_via_tool(args)
        .map_err(|error| ui_error_to_status_message(map_ui_mutation_error(error)))?;
    if let Some(message) = rejected_mutation_message(&result) {
        return Err((StatusCode::UNPROCESSABLE_ENTITY, message));
    }
    Ok(Html(
        render_task_detail_fragment(&state.host, &task_id).map_err(internal_error)?,
    ))
}

async fn console_task_release_lease(
    State(state): State<PrismConsoleState>,
    Path(task_id): Path<String>,
    Form(form): Form<ClaimReleaseForm>,
) -> std::result::Result<impl IntoResponse, (StatusCode, String)> {
    let request = PrismUiMutateRequest {
        action: "claim".to_string(),
        input: json!({
            "action": "release",
            "payload": {
                "claimId": form.claim_id,
            }
        }),
    };
    let args = resolve_ui_mutation_args(&state.root, state.host.workspace_session_ref(), request)
        .map_err(ui_error_to_status_message)?;
    let result = state
        .server
        .execute_prism_mutation_via_tool(args)
        .map_err(|error| ui_error_to_status_message(map_ui_mutation_error(error)))?;
    if let Some(message) = rejected_mutation_message(&result) {
        return Err((StatusCode::UNPROCESSABLE_ENTITY, message));
    }
    Ok(Html(
        render_task_detail_fragment(&state.host, &task_id).map_err(internal_error)?,
    ))
}

async fn console_plan_archive(
    State(state): State<PrismConsoleState>,
    Path(plan_id): Path<String>,
) -> std::result::Result<impl IntoResponse, (StatusCode, String)> {
    let request = PrismUiMutateRequest {
        action: "coordination".to_string(),
        input: json!({
            "kind": "plan_archive",
            "payload": { "planId": plan_id }
        }),
    };
    let args = resolve_ui_mutation_args(&state.root, state.host.workspace_session_ref(), request)
        .map_err(ui_error_to_status_message)?;
    let result = state
        .server
        .execute_prism_mutation_via_tool(args)
        .map_err(|error| ui_error_to_status_message(map_ui_mutation_error(error)))?;
    if let Some(message) = rejected_mutation_message(&result) {
        return Err((StatusCode::UNPROCESSABLE_ENTITY, message));
    }
    let mut headers = HeaderMap::new();
    headers.insert("HX-Redirect", HeaderValue::from_static("/console/plans"));
    Ok((StatusCode::NO_CONTENT, headers))
}

async fn console_concepts_index(
    State(state): State<PrismConsoleState>,
    Query(query): Query<ConceptQuery>,
) -> std::result::Result<Html<String>, (StatusCode, String)> {
    let session = state
        .host
        .ui_session_bootstrap_view()
        .map_err(internal_error)?;
    let entry_concepts = state.host.ui_concept_entrypoints_view().unwrap_or_default();
    let search = query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    let concepts = entry_concepts
        .into_iter()
        .filter(|concept| {
            search.as_ref().map(|needle| {
                concept.canonical_name.to_ascii_lowercase().contains(needle)
                    || concept.handle.to_ascii_lowercase().contains(needle)
            }).unwrap_or(true)
        })
        .map(|concept| {
            format!(
                "<a class=\"console-concept-link\" href=\"/console/concepts/{}\"><strong>{}</strong><span class=\"console-muted console-small\">{}</span></a>",
                escape_html(&concept_handle_to_slug(&concept.handle)),
                escape_html(&concept.canonical_name),
                escape_html(&truncate(&concept.summary, 180)),
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let body = format!(
        "<main class=\"console-layout console-layout--single\">\
         <section class=\"console-card\"><div class=\"console-card-header\"><h2>Concept entrypoints</h2><span class=\"console-sync\">Polling projection-backed concept slices</span></div>\
         <form class=\"console-filter-grid\" method=\"get\" action=\"/console/concepts\">\
         <div class=\"console-field\"><label>Search</label><input class=\"console-input\" type=\"search\" name=\"search\" value=\"{}\" placeholder=\"auth, runtime, coordination\"></div>\
         <div class=\"console-actions\"><button class=\"console-button\" type=\"submit\">Filter</button></div>\
         </form>\
         <div class=\"console-list\">{}</div></section></main>",
        escape_html(query.search.as_deref().unwrap_or("")),
        if concepts.is_empty() {
            "<div class=\"console-empty\">No concepts matched the current filter.</div>".to_string()
        } else {
            concepts
        }
    );
    Ok(Html(page_shell(
        "SSR Concepts",
        "Focused concept pages with bounded Mermaid slices instead of an unreadable whole-repo graph.",
        "concepts",
        &session.session,
        &body,
    )))
}

async fn console_concept_page(
    State(state): State<PrismConsoleState>,
    Path(slug): Path<String>,
    Query(query): Query<ConceptQuery>,
) -> std::result::Result<Html<String>, (StatusCode, String)> {
    let session = state
        .host
        .ui_session_bootstrap_view()
        .map_err(internal_error)?;
    let handle = query
        .handle
        .clone()
        .unwrap_or_else(|| concept_slug_to_handle(&slug));
    let direction = ConceptDirection::parse(query.direction.as_deref());
    let depth = query.depth.unwrap_or(1).clamp(1, 3);
    let prism = state.host.current_prism();
    let slice = build_concept_slice(
        prism.as_ref(),
        &handle,
        depth,
        direction,
        query.relation.as_deref(),
    )
    .ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            format!("concept not found: {handle}"),
        )
    })?;
    let mermaid = concept_graph_mermaid(&slice.focus.handle, &slice.nodes, &slice.edges);
    let relation_html = slice
        .focus
        .relations
        .iter()
        .map(|relation| {
            format!(
                "<li><strong>{}</strong> → <a href=\"/console/concepts/{}?depth={}&direction={}\">{}</a><div class=\"console-muted console-small\">{}</div></li>",
                escape_html(&format!("{:?}", relation.kind)),
                escape_html(&concept_handle_to_slug(&relation.related_handle)),
                slice.depth,
                slice.direction.as_str(),
                escape_html(relation.related_canonical_name.as_deref().unwrap_or("unknown")),
                escape_html(&truncate(relation.related_summary.as_deref().unwrap_or(""), 120))
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let body = format!(
        "<main class=\"console-layout console-layout--single\">\
         <section class=\"console-card\">\
         <div class=\"console-card-header\"><div><p class=\"console-eyebrow\">Focus concept</p><h2>{}</h2></div>{}</div>\
         <form class=\"console-filter-grid\" method=\"get\" action=\"/console/concepts/{}\">\
         <input type=\"hidden\" name=\"handle\" value=\"{}\">\
         <div class=\"console-field\"><label>Depth</label><select class=\"console-select\" name=\"depth\">{}</select></div>\
         <div class=\"console-field\"><label>Direction</label><select class=\"console-select\" name=\"direction\">{}</select></div>\
         <div class=\"console-field\"><label>Relation kind</label><input class=\"console-input\" name=\"relation\" value=\"{}\" placeholder=\"implements, depends_on\"></div>\
         <div class=\"console-actions\"><button class=\"console-button\" type=\"submit\">Refocus slice</button></div>\
         </form>\
         <div class=\"console-grid-two\">\
         <article class=\"console-doc\"><h3>Summary</h3>{}<h3>Neighbor list</h3><ul class=\"console-list\">{}</ul></article>\
         <article class=\"console-card\"><div class=\"console-card-header\"><h3>Concept graph slice</h3><span class=\"console-muted console-small\">Depth {} · {}</span></div><pre class=\"console-mermaid prism-mermaid mermaid\">{}</pre></article>\
        </div></section></main>",
        escape_html(&slice.focus.canonical_name),
        status_badge(&slice.focus.scope_label),
        escape_html(&concept_handle_to_slug(&slice.focus.handle)),
        escape_html(&slice.focus.handle),
        render_depth_options(depth),
        render_direction_options(direction),
        escape_html(slice.relation_filter.as_deref().unwrap_or("")),
        markdown_to_html(&slice.focus.summary),
        if relation_html.is_empty() {
            "<li class=\"console-empty\">No matching neighbors in the current bounded slice.</li>"
                .to_string()
        } else {
            relation_html
        },
        depth,
        escape_html(direction.as_str()),
        escape_html(&mermaid),
    );
    Ok(Html(page_shell(
        "SSR Concept Detail",
        "Incremental concept exploration with bounded graph slices and textual neighborhood navigation.",
        "concepts",
        &session.session,
        &body,
    )))
}

async fn console_fleet_page(
    State(state): State<PrismConsoleState>,
) -> std::result::Result<Html<String>, (StatusCode, String)> {
    let session = state
        .host
        .ui_session_bootstrap_view()
        .map_err(internal_error)?;
    let body = format!(
        "<main class=\"console-layout console-layout--single\">\
         <section id=\"console-fleet\" class=\"console-live\" hx-get=\"/console/fragments/fleet\" hx-trigger=\"load, every 2s\" hx-swap=\"outerHTML\">\
         {}\
         </section></main>",
        render_fleet_fragment(&state.host).map_err(internal_error)?
    );
    Ok(Html(page_shell(
        "SSR Fleet",
        "vis-timeline swimlanes for runtimes, claims, and suspiciously long-running work.",
        "fleet",
        &session.session,
        &body,
    )))
}

async fn console_fleet_fragment(
    State(state): State<PrismConsoleState>,
) -> std::result::Result<Html<String>, (StatusCode, String)> {
    render_fleet_fragment(&state.host)
        .map(Html)
        .map_err(internal_error)
}

fn render_plans_workspace(host: &QueryHost, query: PlansQuery) -> Result<String> {
    let view = host.ui_plans_view(UiPlansQueryOptions {
        selected_plan_id: query.plan_id.clone(),
        status: query.status.clone(),
        search: query.search.clone(),
        sort: query.sort.clone(),
        agent: query.agent.clone(),
    })?;
    let sidebar = render_plan_sidebar(&view);
    let detail = view
        .selected_plan
        .as_ref()
        .map(render_plan_detail)
        .unwrap_or_else(|| {
            "<div class=\"console-empty\">No plan matches the current filters.</div>".to_string()
        });
    Ok(format!(
        "<section id=\"console-plans-workspace\" class=\"console-layout console-layout--two\">\
         <aside class=\"console-sidebar\">{}\
         </aside><section class=\"console-main\">{}\
         </section></section>",
        sidebar, detail
    ))
}

fn render_plan_sidebar(view: &PrismPlansView) -> String {
    let items = view
        .plans
        .iter()
        .map(|plan| {
            let progress = percent(
                plan.plan_summary.completed_nodes,
                plan.plan_summary.total_nodes,
            );
            format!(
                "<a class=\"console-plan-link\" href=\"/console/plans/{}\" data-selected=\"{}\">\
                 <div class=\"console-card-header\"><strong>{}</strong>{}</div>\
                 <div class=\"console-progress\"><span style=\"width:{}%\"></span></div>\
                 <div class=\"console-inline-list\"><span class=\"console-pill\">{}% complete</span><span class=\"console-pill\">{} active</span></div></a>",
                escape_html(&plan.plan_id),
                if view.selected_plan_id.as_deref() == Some(plan.plan_id.as_str()) { "true" } else { "false" },
                escape_html(&plan.title),
                status_badge(&format!("{:?}", plan.status)),
                progress,
                progress,
                plan.plan_summary.in_progress_nodes,
            )
        })
        .collect::<Vec<_>>()
        .join("");
    format!(
        "<section class=\"console-sidebar-card\">\
         <div class=\"console-card-header\"><div><p class=\"console-eyebrow\">Portfolio</p><h2>Plans</h2></div><span class=\"console-muted console-small\">{}</span></div>\
         <form class=\"console-filter-grid\" method=\"get\" action=\"/console/plans\">\
         <div class=\"console-field\"><label>Status</label><select class=\"console-select\" name=\"status\">{}</select></div>\
         <div class=\"console-field\"><label>Sort</label><select class=\"console-select\" name=\"sort\">{}</select></div>\
         <div class=\"console-field\"><label>Search</label><input class=\"console-input\" type=\"search\" name=\"search\" value=\"{}\" placeholder=\"plan title or goal\"></div>\
         <div class=\"console-field\"><label>Agent</label><input class=\"console-input\" name=\"agent\" value=\"{}\" placeholder=\"runtime or agent\"></div>\
         <div class=\"console-actions\"><button class=\"console-button\" type=\"submit\">Apply</button></div>\
         </form>\
         <div class=\"console-list\">{}</div>\
         </section>",
        view.stats.visible_plans,
        render_select_options(
            &[
                ("active", "Active"),
                ("completed", "Completed"),
                ("archived", "Archived"),
                ("blocked", "Blocked"),
                ("abandoned", "Abandoned"),
                ("draft", "Draft"),
                ("all", "All"),
            ],
            &view.filters.status
        ),
        render_select_options(
            &[
                ("newest", "Newest first"),
                ("oldest", "Oldest first"),
                ("priority", "Priority"),
                ("actionable", "Most actionable"),
                ("completion", "Completion"),
                ("title", "Title")
            ],
            &view.filters.sort
        ),
        escape_html(view.filters.search.as_deref().unwrap_or("")),
        escape_html(view.filters.agent.as_deref().unwrap_or("")),
        if items.is_empty() {
            "<div class=\"console-empty\">No plans matched the current filter set.</div>"
                .to_string()
        } else {
            items
        }
    )
}

fn render_plan_detail(view: &PrismPlanDetailView) -> String {
    let markdown_url = format!(
        "/console/plans/{}/markdown",
        escape_html(&view.plan.plan_id)
    );
    let child_plan_rows = view
        .child_plans
        .iter()
        .map(|plan| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape_html(&plan.title),
                status_badge(&format!("{:?}", plan.status)),
                escape_html(&plan.id)
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let child_task_rows = view
        .child_tasks
        .iter()
        .map(|task| {
            format!(
                "<tr><td><a href=\"/console/tasks/{}\">{}</a></td><td>{}</td><td>{}</td></tr>",
                escape_html(&task.id),
                escape_html(&task.title),
                status_badge(&format!("{:?}", task.status)),
                escape_html(task.assignee.as_deref().unwrap_or("unassigned"))
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let ready_rows = view
        .ready_tasks
        .iter()
        .map(|task| {
            format!(
                "<tr><td><a href=\"/console/tasks/{}\">{}</a></td><td>{}</td><td>{}</td></tr>",
                escape_html(&task.id),
                escape_html(&task.title),
                status_badge(&format!("{:?}", task.status)),
                escape_html(task.assignee.as_deref().unwrap_or("unassigned"))
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let outcomes = view
        .recent_outcomes
        .iter()
        .map(|outcome| {
            format!(
                "<li><strong>{}</strong><div class=\"console-muted console-small\">{}</div></li>",
                escape_html(&outcome.kind),
                escape_html(&truncate(&outcome.summary, 140))
            )
        })
        .collect::<Vec<_>>()
        .join("");
    format!(
        "<article class=\"console-card\">\
         <div class=\"console-card-header\">\
         <div><p class=\"console-eyebrow\">Plan detail</p><h2>{}</h2><p class=\"console-subtitle\">{}</p></div>\
         <div class=\"console-actions\">\
         <a class=\"console-button console-button--ghost\" href=\"{}\" download>Download markdown</a>\
         <div class=\"console-copy-action\"><button class=\"console-button console-button--ghost\" type=\"button\" data-copy-markdown-url=\"{}\"><span class=\"console-action-label\">Copy markdown</span><span class=\"console-action-spinner\" aria-hidden=\"true\"></span></button><span class=\"console-action-feedback console-small\" data-copy-markdown-feedback aria-live=\"polite\"></span></div>\
         <form class=\"console-action-form\" hx-post=\"/console/plans/{}/archive\" hx-swap=\"none\" hx-indicator=\"closest .console-action-form\"><button class=\"console-button console-button--warn\" type=\"submit\"><span class=\"console-action-label\">Archive plan</span><span class=\"console-action-spinner\" aria-hidden=\"true\"></span></button></form></div>\
         </div>\
         <div class=\"console-inline-list\">{}<span class=\"console-pill\">{} total nodes</span><span class=\"console-pill\">{} actionable</span><span class=\"console-pill\">{} direct children</span></div>\
         <section class=\"console-card\"><div class=\"console-card-header\"><div><h3>Plan structure</h3><p class=\"console-subtitle\">Canonical child plans and child tasks for this plan.</p></div><span class=\"console-sync\">Live via polling</span></div>\
         <div class=\"console-grid console-grid--two\">\
         <div><h4>Child plans</h4><table class=\"console-data-table\"><thead><tr><th>Plan</th><th>Status</th><th>Id</th></tr></thead><tbody>{}</tbody></table></div>\
         <div><h4>Child tasks</h4><table class=\"console-data-table\"><thead><tr><th>Task</th><th>Status</th><th>Assignee</th></tr></thead><tbody>{}</tbody></table></div>\
         </div></section>\
         <section class=\"console-card\"><div class=\"console-card-header\"><h3>Ready tasks</h3><span class=\"console-muted console-small\">{}</span></div>\
         <table class=\"console-data-table\"><thead><tr><th>Task</th><th>Status</th><th>Assignee</th></tr></thead><tbody>{}</tbody></table></section>\
         <section class=\"console-card\"><div class=\"console-card-header\"><h3>Recent outcomes</h3></div><ul class=\"console-list\">{}</ul></section>\
         </article>",
        escape_html(&view.plan.title),
        escape_html(&view.plan.goal),
        markdown_url,
        markdown_url,
        escape_html(&view.plan.plan_id),
        status_badge(&format!("{:?}", view.plan.status)),
        view.summary.total_nodes,
        view.summary.actionable_nodes,
        view.children.len(),
        if child_plan_rows.is_empty() {
            "<tr><td colspan=\"3\"><div class=\"console-empty\">No child plans.</div></td></tr>"
                .to_string()
        } else {
            child_plan_rows
        },
        if child_task_rows.is_empty() {
            "<tr><td colspan=\"3\"><div class=\"console-empty\">No direct child tasks.</div></td></tr>"
                .to_string()
        } else {
            child_task_rows
        },
        view.ready_tasks.len(),
        if ready_rows.is_empty() {
            "<tr><td colspan=\"3\"><div class=\"console-empty\">No ready tasks right now.</div></td></tr>".to_string()
        } else {
            ready_rows
        },
        if outcomes.is_empty() {
            "<li class=\"console-empty\">No recent outcomes recorded for this plan yet.</li>"
                .to_string()
        } else {
            outcomes
        }
    )
}

fn render_task_detail_fragment(host: &QueryHost, task_id: &str) -> Result<String> {
    let view = host
        .ui_task_detail_view(task_id)?
        .ok_or_else(|| anyhow!("task not found: {task_id}"))?;
    let active_claim = view
        .claim_history
        .iter()
        .find(|claim| claim.status.eq_ignore_ascii_case("active"));
    let claim_rows = view
        .claim_history
        .iter()
        .map(|claim| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape_html(&claim.holder),
                status_badge(&claim.status),
                escape_html(claim.branch_ref.as_deref().unwrap_or("unknown")),
                escape_html(&duration_label(claim.duration_seconds))
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let blocker_rows = view
        .blockers
        .iter()
        .map(|entry| {
            let related = entry
                .related_task
                .as_ref()
                .map(|task| {
                    format!(
                        "<a href=\"/console/tasks/{}\">{}</a>",
                        escape_html(&task.id),
                        escape_html(&task.title)
                    )
                })
                .unwrap_or_else(|| "unknown".to_string());
            format!(
                "<tr><td>{}</td><td>{}</td></tr>",
                escape_html(&entry.blocker.summary),
                related
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let outcome_rows = view
        .outcomes
        .iter()
        .map(|outcome| {
            format!(
                "<li><strong>{}</strong><div class=\"console-muted console-small\">{}</div></li>",
                escape_html(&outcome.kind),
                escape_html(&truncate(&outcome.summary, 160))
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let commit_rows = view
        .recent_commits
        .iter()
        .map(|commit| {
            format!(
                "<li><strong>{}</strong><div class=\"console-muted console-small\">{}</div></li>",
                escape_html(&commit.label),
                escape_html(&commit.commit)
            )
        })
        .collect::<Vec<_>>()
        .join("");
    Ok(format!(
        "<section id=\"console-task-detail\" class=\"console-task-card\">\
         <div class=\"console-card-header\"><div><p class=\"console-eyebrow\">Task detail</p><h2>{}</h2></div>{}</div>\
         <div class=\"console-grid-two\">\
         <article class=\"console-card\">\
         <div class=\"console-card-header\"><h3>Edit metadata</h3><span class=\"console-sync\">Syncing…</span></div>\
         <form class=\"console-stack\" hx-post=\"/console/tasks/{}/edit\" hx-target=\"#console-task-detail\" hx-swap=\"outerHTML\">\
         <div class=\"console-field\"><label>Title</label><input class=\"console-input\" name=\"title\" value=\"{}\"></div>\
         <div class=\"console-field\"><label>Description</label><textarea class=\"console-textarea\" name=\"description\">{}</textarea></div>\
         <div class=\"console-filter-grid\">\
         <div class=\"console-field\"><label>Priority</label><input class=\"console-input\" name=\"priority\" value=\"{}\" placeholder=\"1-255\"></div>\
         <div class=\"console-field\"><label>Assignee</label><input class=\"console-input\" name=\"assignee\" value=\"{}\"></div>\
         <div class=\"console-field\"><label>Status</label><select class=\"console-select\" name=\"status\">{}</select></div>\
         </div>\
         <div class=\"console-actions\"><button class=\"console-button\" type=\"submit\">Save task</button></div>\
         </form>\
         </article>\
         <article class=\"console-card\">\
         <div class=\"console-card-header\"><h3>Operator actions</h3></div>\
         <div class=\"console-stack\">\
         <div class=\"console-inline-list\">{}<span class=\"console-pill\">validation refs: {}</span></div>\
         {}\
         </div></article></div>\
         <section class=\"console-grid-two\">\
         <article class=\"console-card\"><div class=\"console-card-header\"><h3>Claim history</h3></div><table class=\"console-data-table\"><thead><tr><th>Holder</th><th>Status</th><th>Branch</th><th>Duration</th></tr></thead><tbody>{}</tbody></table></article>\
         <article class=\"console-card\"><div class=\"console-card-header\"><h3>Blockers</h3></div><table class=\"console-data-table\"><thead><tr><th>Reason</th><th>Related task</th></tr></thead><tbody>{}</tbody></table></article>\
         </section>\
         <section class=\"console-grid-two\">\
         <article class=\"console-card\"><div class=\"console-card-header\"><h3>Outcomes</h3></div><ul class=\"console-list\">{}</ul></article>\
         <article class=\"console-card\"><div class=\"console-card-header\"><h3>Recent commits</h3></div><ul class=\"console-list\">{}</ul></article>\
         </section></section>",
        escape_html(&view.task.title),
        status_badge(&format!("{:?}", view.task.status)),
        escape_html(&view.task.id),
        escape_html(&view.editable.title),
        escape_html(view.editable.description.as_deref().unwrap_or("")),
        view.editable.priority.map(|value| value.to_string()).unwrap_or_default(),
        escape_html(view.editable.assignee.as_deref().unwrap_or("")),
        render_select_options(
            &view
                .editable
                .status_options
                .iter()
                .map(|status| (status.as_str(), status.as_str()))
                .collect::<Vec<_>>(),
            &status_slug(&view.editable.status)
        ),
        status_badge(&format!("{:?}", view.task.status)),
        view.editable.validation_refs.len(),
        active_claim
            .map(|claim| {
                format!(
                    "<form hx-post=\"/console/tasks/{}/release-lease\" hx-target=\"#console-task-detail\" hx-swap=\"outerHTML\">\
                     <input type=\"hidden\" name=\"claim_id\" value=\"{}\">\
                     <button class=\"console-button console-button--warn\" type=\"submit\">Revoke lease</button></form>",
                    escape_html(&view.task.id),
                    escape_html(&claim.id),
                )
            })
            .unwrap_or_else(|| "<div class=\"console-empty\">No active lease to revoke.</div>".to_string()),
        if claim_rows.is_empty() {
            "<tr><td colspan=\"4\"><div class=\"console-empty\">No claim history.</div></td></tr>".to_string()
        } else {
            claim_rows
        },
        if blocker_rows.is_empty() {
            "<tr><td colspan=\"2\"><div class=\"console-empty\">No blockers are currently recorded.</div></td></tr>".to_string()
        } else {
            blocker_rows
        },
        if outcome_rows.is_empty() {
            "<li class=\"console-empty\">No outcomes recorded yet.</li>".to_string()
        } else {
            outcome_rows
        },
        if commit_rows.is_empty() {
            "<li class=\"console-empty\">No commits linked yet.</li>".to_string()
        } else {
            commit_rows
        }
    ))
}

fn render_fleet_fragment(host: &QueryHost) -> Result<String> {
    let fleet = host.ui_fleet_view()?;
    let lanes = fleet
        .lanes
        .iter()
        .map(|lane| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape_html(&lane.label),
                escape_html(lane.branch_ref.as_deref().unwrap_or("unknown")),
                lane.active_bar_count,
                if lane.idle { "idle" } else { "active" }
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let timeline_payload = json!({
        "groups": fleet
            .lanes
            .iter()
            .map(|lane| json!({
                "id": lane.id,
                "content": lane.label,
                "title": lane.principal_id,
            }))
            .collect::<Vec<_>>(),
        "items": fleet
            .bars
            .iter()
            .map(|bar| json!({
                "id": bar.id,
                "group": bar.lane_id,
                "content": format!("{} · {}", bar.task_title, if bar.active { "active" } else { "settled" }),
                "start": bar.started_at * 1000,
                "end": bar.ended_at.unwrap_or(fleet.window_end) * 1000,
                "title": format!("{} ({})", bar.task_title, duration_label(bar.duration_seconds)),
                "taskUrl": bar.task_id.as_ref().map(|task_id| format!("/console/tasks/{task_id}")),
                "className": if bar.stale { "fleet-stale" } else if bar.active { "fleet-active" } else { "fleet-complete" },
            }))
            .collect::<Vec<_>>()
    });
    Ok(format!(
        "<section id=\"console-fleet\" class=\"console-card\">\
         <div class=\"console-card-header\"><div><p class=\"console-eyebrow\">Fleet timeline</p><h2>Runtime utilization</h2></div><span class=\"console-sync\">Polling every 2s</span></div>\
         <div class=\"console-fleet-host\" data-prism-fleet-host><script type=\"application/json\">{}</script></div>\
         <div class=\"console-grid-two\">\
         <article class=\"console-card\"><div class=\"console-card-header\"><h3>Lane summary</h3></div><table class=\"console-data-table\"><thead><tr><th>Runtime</th><th>Branch</th><th>Bars</th><th>State</th></tr></thead><tbody>{}</tbody></table></article>\
         <article class=\"console-card\"><div class=\"console-card-header\"><h3>Window</h3></div><ul class=\"console-list\"><li>Runtimes: <strong>{}</strong></li><li>Bars: <strong>{}</strong></li><li>Window seconds: <strong>{}</strong></li></ul></article>\
         </div></section>",
        json_script_escape(&timeline_payload.to_string()),
        if lanes.is_empty() {
            "<tr><td colspan=\"4\"><div class=\"console-empty\">No runtimes are currently visible.</div></td></tr>".to_string()
        } else {
            lanes
        },
        fleet.lanes.len(),
        fleet.bars.len(),
        fleet.window_end.saturating_sub(fleet.window_start),
    ))
}

fn render_overview_plan_list(view: &PrismPlansView) -> String {
    let items = view
        .plans
        .iter()
        .take(5)
        .map(|plan| {
            format!(
                "<li><a href=\"/console/plans/{}\">{}</a><div class=\"console-muted console-small\">{} · {}% complete</div></li>",
                escape_html(&plan.plan_id),
                escape_html(&plan.title),
                escape_html(&format!("{:?}", plan.status)),
                percent(plan.plan_summary.completed_nodes, plan.plan_summary.total_nodes)
            )
        })
        .collect::<Vec<_>>()
        .join("");
    if items.is_empty() {
        "<div class=\"console-empty\">No plans are currently visible.</div>".to_string()
    } else {
        format!("<ul class=\"console-list\">{items}</ul>")
    }
}

fn render_overview_fleet_list(view: &PrismUiFleetView) -> String {
    let items = view
        .lanes
        .iter()
        .take(5)
        .map(|lane| {
            format!(
                "<li><strong>{}</strong><div class=\"console-muted console-small\">{} active bars · branch {}</div></li>",
                escape_html(&lane.label),
                lane.active_bar_count,
                escape_html(lane.branch_ref.as_deref().unwrap_or("unknown"))
            )
        })
        .collect::<Vec<_>>()
        .join("");
    if items.is_empty() {
        "<div class=\"console-empty\">No runtimes are currently visible.</div>".to_string()
    } else {
        format!("<ul class=\"console-list\">{items}</ul>")
    }
}

fn render_select_options(options: &[(&str, &str)], selected: &str) -> String {
    let selected_key = comparable_status_key(selected);
    options
        .iter()
        .map(|(value, label)| {
            let value_key = comparable_status_key(value);
            format!(
                "<option value=\"{}\"{}>{}</option>",
                escape_html(value),
                if selected == *value || selected_key == value_key {
                    " selected"
                } else {
                    ""
                },
                escape_html(label)
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

fn comparable_status_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

fn render_depth_options(selected: usize) -> String {
    (1..=3)
        .map(|depth| {
            format!(
                "<option value=\"{}\"{}>{}</option>",
                depth,
                if depth == selected { " selected" } else { "" },
                depth
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

fn render_direction_options(selected: ConceptDirection) -> String {
    [
        (ConceptDirection::Both, "both"),
        (ConceptDirection::Outbound, "outbound"),
        (ConceptDirection::Inbound, "inbound"),
    ]
    .into_iter()
    .map(|(value, label)| {
        format!(
            "<option value=\"{}\"{}>{}</option>",
            label,
            if value == selected { " selected" } else { "" },
            label
        )
    })
    .collect::<Vec<_>>()
    .join("")
}

fn sparse_patch(value: Option<String>) -> serde_json::Value {
    match value.map(|value| value.trim().to_string()) {
        Some(value) if !value.is_empty() => json!({ "op": "set", "value": value }),
        _ => json!({ "op": "clear" }),
    }
}

fn sparse_u8_patch(value: Option<&str>) -> serde_json::Value {
    match value.and_then(|value| value.trim().parse::<u8>().ok()) {
        Some(value) => json!({ "op": "set", "value": value }),
        None => json!({ "op": "clear" }),
    }
}

fn plans_fragment_url(query: &PlansQuery, selected_plan_id: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(plan_id) = selected_plan_id.or(query.plan_id.as_deref()) {
        parts.push(format!("planId={plan_id}"));
    }
    if let Some(status) = &query.status {
        parts.push(format!("status={status}"));
    }
    if let Some(search) = &query.search {
        parts.push(format!("search={search}"));
    }
    if let Some(sort) = &query.sort {
        parts.push(format!("sort={sort}"));
    }
    if let Some(agent) = &query.agent {
        parts.push(format!("agent={agent}"));
    }
    if parts.is_empty() {
        "/console/fragments/plans".to_string()
    } else {
        format!("/console/fragments/plans?{}", parts.join("&"))
    }
}

fn plan_markdown_payload(host: &QueryHost, plan_id: &str) -> Result<Option<(String, String)>> {
    let prism = host.current_prism();
    let plan_id = PlanId::new(plan_id.to_string());
    let Some(plan) = prism.coordination_plan_v2(&plan_id) else {
        return Ok(None);
    };
    let status = prism
        .coordination_snapshot()
        .plans
        .into_iter()
        .find(|plan| plan.id == plan_id)
        .map(|plan| plan.status);
    let markdown = render_repo_published_plan_markdown(
        &prism.coordination_snapshot_v2(),
        &plan_id,
        status,
    )
    .ok_or_else(|| anyhow!("plan markdown should be renderable for {}", plan_id.0))?;
    Ok(Some((plan.plan.title, markdown)))
}

fn sanitize_download_basename(value: &str) -> String {
    let mut sanitized = value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | '0'..='9' => ch,
            'A'..='Z' => ch.to_ascii_lowercase(),
            _ => '-',
        })
        .collect::<String>();
    while sanitized.contains("--") {
        sanitized = sanitized.replace("--", "-");
    }
    let trimmed = sanitized.trim_matches('-');
    if trimmed.is_empty() {
        "plan".to_string()
    } else {
        trimmed.to_string()
    }
}

fn internal_error(error: anyhow::Error) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

fn ui_error_to_status_message(
    error: (StatusCode, axum::Json<serde_json::Value>),
) -> (StatusCode, String) {
    (error.0, error.1 .0.to_string())
}

fn rejected_mutation_message(result: &crate::PrismMutationResult) -> Option<String> {
    let payload = result.result.as_object()?;
    if !payload
        .get("rejected")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    payload
        .get("violations")
        .and_then(serde_json::Value::as_array)
        .and_then(|violations| violations.first())
        .and_then(|violation| violation.get("summary"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .or_else(|| Some("operator console mutation was rejected".to_string()))
}

async fn prism_ui_favicon(
    State(state): State<PrismUiState>,
) -> std::result::Result<Response<Body>, (StatusCode, String)> {
    let (bytes, mime) = prism_ui_favicon_asset(&state.root)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, mime)
        .body(Body::from(bytes))
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use prism_coordination::CoordinationPolicy;
    use prism_ir::{EventActor, EventId, EventMeta, TaskId};
    use tower::util::ServiceExt;

    use crate::tests_support::temp_workspace;
    use prism_core::index_workspace_session;

    fn console_state_from_root(root: &std::path::Path) -> PrismConsoleState {
        let server = Arc::new(PrismMcpServer::with_session(
            index_workspace_session(root).unwrap(),
        ));
        let host = Arc::clone(&server.host);
        PrismConsoleState {
            server,
            host,
            root: root.to_path_buf(),
        }
    }

    fn console_state_with_plan() -> (PrismConsoleState, String) {
        let root = temp_workspace();
        let state = console_state_from_root(&root);
        let plan_id = state
            .host
            .current_prism()
            .create_native_plan(
                EventMeta {
                    id: EventId::new("coordination:console-plan-markdown"),
                    ts: 1,
                    actor: EventActor::Agent,
                    correlation: Some(TaskId::new("task:console-plan-markdown")),
                    causation: None,
                    execution_context: None,
                },
                "Console markdown export".into(),
                "Make plan markdown available from the SSR plans page.".into(),
                None,
                Some(CoordinationPolicy::default()),
            )
            .unwrap();
        (state, plan_id.0.to_string())
    }

    #[tokio::test]
    async fn console_routes_render_primary_pages() {
        let root = temp_workspace();
        let router = routes(console_state_from_root(&root));

        for path in [
            "/console",
            "/console/plans",
            "/console/concepts",
            "/console/fleet",
        ] {
            let response = router
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let text = std::str::from_utf8(&body).unwrap();
            assert!(text.contains("PRISM SSR Console"), "{path}");
            assert!(text.contains("/console/favicon.svg"), "{path}");
        }
    }

    #[tokio::test]
    async fn console_routes_serve_local_assets() {
        let root = temp_workspace();
        let router = routes(console_state_from_root(&root));

        for path in [
            "/console/assets/console.css",
            "/console/assets/console.js",
            "/console/favicon.svg",
        ] {
            let response = router
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
        }
    }

    #[tokio::test]
    async fn console_plan_markdown_route_returns_attachment() {
        let (state, plan_id) = console_state_with_plan();
        let router = routes(state);

        let response = router
            .oneshot(
                Request::builder()
                    .uri(format!("/console/plans/{plan_id}/markdown"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CONTENT_TYPE).unwrap(),
            "text/markdown; charset=utf-8"
        );
        assert!(response
            .headers()
            .get(CONTENT_DISPOSITION)
            .unwrap()
            .to_str()
            .unwrap()
            .contains(".md"));
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = std::str::from_utf8(&body).unwrap();
        assert!(text.contains("# Console markdown export"));
        assert!(text.contains("## Goal"));
        assert!(text.contains("## Git Execution Policy"));
    }

    #[tokio::test]
    async fn console_plan_detail_renders_markdown_actions() {
        let (state, plan_id) = console_state_with_plan();
        let router = routes(state);

        let response = router
            .oneshot(
                Request::builder()
                    .uri(format!("/console/plans/{plan_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = std::str::from_utf8(&body).unwrap();
        assert!(text.contains("Download markdown"));
        assert!(text.contains("Copy markdown"));
        assert!(text.contains(&format!("/console/plans/{plan_id}/markdown")));
        assert!(text.contains("data-copy-markdown-url"));
    }
}
