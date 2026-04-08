use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use prism_core::runtime_engine::{
    RuntimeFreshnessState, RuntimeMaterializationDepth, WorkspacePublishedGeneration,
    WorkspaceRuntimeQueueSnapshot,
};
use prism_core::{
    assisted_lease_renewal_diagnostics, shared_coordination_ref_diagnostics, PrismPaths,
    WorkspaceSession,
};
use prism_js::{
    ConnectionInfoView, ProjectionAuthorityPlaneView, ProjectionClassView,
    ProjectionFreshnessStateView, ProjectionMaterializationStateView, ProjectionReadModelView,
    RuntimeAssistedLeaseRenewalView, RuntimeBoundaryRegionView, RuntimeCoordinationLagView,
    RuntimeCoordinationSurfaceLagItemView, RuntimeDescriptorCapabilityView,
    RuntimeDiscoveryModeView, RuntimeDomainFreshnessView, RuntimeFreshnessView, RuntimeHealthView,
    RuntimeLogEventView, RuntimeMaterializationCoverageView, RuntimeMaterializationItemView,
    RuntimeMaterializationView, RuntimeOverlayScopeView, RuntimeProcessView,
    RuntimeProjectionScopeView, RuntimeQueueDepthView, RuntimeScopesView,
    RuntimeSharedCoordinationRefView, RuntimeSharedCoordinationRuntimeDescriptorView,
    RuntimeStatusView,
};
use prism_projections::{
    ProjectionAuthorityPlane, ProjectionClass, ProjectionFreshnessState,
    ProjectionMaterializationState, ProjectionReadModel, ProjectionScopeReadModel,
};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::daemon_log;
use crate::diagnostics_state::RuntimeStatusRevisionKey;
use crate::log_scope::{select_log_sources, LogScope, RepoLogSource};
use crate::mcp_call_log::McpCallLogStore;
use crate::runtime_state::{
    process_is_live, read_runtime_state, RuntimeEventRecord, RuntimeProcessRecord, RuntimeState,
};
use crate::serving_projection_models::runtime_projection_scopes;
use crate::workspace_diagnostics::WorkspaceDiagnosticsConfig;
use crate::{QueryHost, RuntimeLogArgs, RuntimeTimelineArgs};

const DEFAULT_HEALTH_PATH: &str = "/healthz";
const DEFAULT_RUNTIME_LOG_LIMIT: usize = 50;
const DEFAULT_RUNTIME_TIMELINE_LIMIT: usize = 20;
const DEFAULT_LOG_SCAN_LINES: usize = 400;
const MAX_LOG_SCAN_LINES: usize = 4_000;
#[derive(Debug, Clone)]
struct RuntimePaths {
    uri_file: PathBuf,
    log_path: PathBuf,
    cache_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum McpProcessKind {
    Daemon,
    Bridge,
}

#[derive(Debug, Clone)]
struct McpProcess {
    pid: u32,
    ppid: u32,
    rss_kb: u64,
    elapsed: String,
    command: String,
    kind: McpProcessKind,
    health_path: Option<String>,
    http_uri: Option<String>,
    upstream_uri: Option<String>,
}

#[derive(Debug, Clone)]
struct ProcessSnapshot {
    pid: u32,
    ppid: u32,
    rss_kb: u64,
    elapsed: String,
    command: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BridgeState {
    Connected,
    Idle,
    Orphaned,
}

#[derive(Debug, Clone, Default)]
struct BridgeCounts {
    connected: usize,
    idle: usize,
    orphaned: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct DaemonLogRecord {
    timestamp: Option<String>,
    level: Option<String>,
    message: Option<String>,
    target: Option<String>,
    filename: Option<String>,
    line_number: Option<u64>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

pub(crate) fn runtime_status(host: &QueryHost) -> Result<RuntimeStatusView> {
    let revisions = RuntimeStatusRevisionKey {
        workspace_revision: host.loaded_workspace_revision_value(),
        episodic_revision: host.loaded_episodic_revision_value(),
        inference_revision: host.loaded_inference_revision_value(),
        coordination_revision: host.loaded_coordination_revision_value(),
    };
    if let Some(cached) = host
        .diagnostics_state()
        .runtime_status_for_revisions(revisions)
    {
        return Ok(cached);
    }
    refresh_cached_runtime_status(host)
}

pub(crate) fn refresh_cached_runtime_status(host: &QueryHost) -> Result<RuntimeStatusView> {
    let workspace = host.workspace_session().ok_or_else(|| {
        anyhow!("runtime introspection requires a workspace-backed PRISM session")
    })?;
    let binding = host.workspace_runtime_binding_ref().ok_or_else(|| {
        anyhow!("runtime introspection requires a workspace-backed PRISM session")
    })?;
    let inputs = RuntimeStatusInputs {
        root: workspace.root(),
        workspace: workspace.as_ref(),
        prism: host.current_prism(),
        loaded_workspace_revision: host.loaded_workspace_revision_value(),
        loaded_episodic_revision: host.loaded_episodic_revision_value(),
        loaded_inference_revision: host.loaded_inference_revision_value(),
        loaded_coordination_revision: host.loaded_coordination_revision_value(),
        published_generation: Some(binding.published_generation_snapshot()),
        queue_snapshot: Some(binding.runtime().queue_snapshot()),
        mcp_call_log_store: host.mcp_call_log_store.as_ref(),
    };
    let runtime_state = read_runtime_state(inputs.root)?;
    let last_runtime_event = latest_runtime_state_event_view(runtime_state.as_ref());
    let cached_shared_coordination_ref = host
        .diagnostics_state()
        .shared_coordination_ref_for_revision(inputs.loaded_coordination_revision);
    let status = runtime_status_from_inputs(
        &inputs,
        runtime_state.as_ref(),
        cached_shared_coordination_ref,
    )?;
    host.diagnostics_state().update_runtime_status(
        status.clone(),
        last_runtime_event,
        RuntimeStatusRevisionKey {
            workspace_revision: inputs.loaded_workspace_revision,
            episodic_revision: inputs.loaded_episodic_revision,
            inference_revision: inputs.loaded_inference_revision,
            coordination_revision: inputs.loaded_coordination_revision,
        },
    );
    Ok(status)
}

pub(crate) fn refresh_cached_runtime_status_for_config(
    config: &WorkspaceDiagnosticsConfig,
) -> Result<RuntimeStatusView> {
    let published_generation = config
        .runtime_engine
        .lock()
        .expect("workspace runtime engine lock poisoned")
        .published_generation_snapshot();
    let queue_snapshot = config
        .runtime_engine
        .lock()
        .expect("workspace runtime engine lock poisoned")
        .queue_snapshot();
    let inputs = RuntimeStatusInputs {
        root: config.workspace.root(),
        workspace: config.workspace.as_ref(),
        prism: config.workspace.prism_arc(),
        loaded_workspace_revision: config.loaded_workspace_revision.load(Ordering::Relaxed),
        loaded_episodic_revision: config.loaded_episodic_revision.load(Ordering::Relaxed),
        loaded_inference_revision: config.loaded_inference_revision.load(Ordering::Relaxed),
        loaded_coordination_revision: config.loaded_coordination_revision.load(Ordering::Relaxed),
        published_generation: Some(published_generation),
        queue_snapshot: Some(queue_snapshot),
        mcp_call_log_store: config.mcp_call_log_store.as_ref(),
    };
    let runtime_state = read_runtime_state(inputs.root)?;
    let last_runtime_event = latest_runtime_state_event_view(runtime_state.as_ref());
    let cached_shared_coordination_ref = config
        .diagnostics_state
        .shared_coordination_ref_for_revision(inputs.loaded_coordination_revision);
    let status = runtime_status_from_inputs(
        &inputs,
        runtime_state.as_ref(),
        cached_shared_coordination_ref,
    )?;
    config.diagnostics_state.update_runtime_status(
        status.clone(),
        last_runtime_event,
        RuntimeStatusRevisionKey {
            workspace_revision: inputs.loaded_workspace_revision,
            episodic_revision: inputs.loaded_episodic_revision,
            inference_revision: inputs.loaded_inference_revision,
            coordination_revision: inputs.loaded_coordination_revision,
        },
    );
    Ok(status)
}

struct RuntimeStatusInputs<'a> {
    root: &'a Path,
    workspace: &'a WorkspaceSession,
    prism: std::sync::Arc<prism_query::Prism>,
    loaded_workspace_revision: u64,
    loaded_episodic_revision: u64,
    loaded_inference_revision: u64,
    loaded_coordination_revision: u64,
    published_generation: Option<WorkspacePublishedGeneration>,
    queue_snapshot: Option<WorkspaceRuntimeQueueSnapshot>,
    mcp_call_log_store: &'a McpCallLogStore,
}

pub(crate) fn connection_info(host: &QueryHost) -> Result<ConnectionInfoView> {
    let root = workspace_root(host)?;
    let paths = RuntimePaths::for_root(root)?;
    let runtime_state = read_runtime_state(root)?;
    let state_processes = runtime_state
        .as_ref()
        .map(|state| state.processes.as_slice())
        .unwrap_or(&[]);
    let (processes, process_error) = match list_runtime_processes(root, state_processes) {
        Ok(processes) => (processes, None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    };
    let daemons = select_kind(&processes, McpProcessKind::Daemon);
    daemon_connection_info(root, &paths, &daemons, process_error.as_deref())
}

pub(crate) fn runtime_logs(
    host: &QueryHost,
    args: RuntimeLogArgs,
) -> Result<Vec<RuntimeLogEventView>> {
    let root = workspace_root(host)?;
    let limit = args.limit.unwrap_or(DEFAULT_RUNTIME_LOG_LIMIT);
    if limit == 0 {
        return Ok(Vec::new());
    }
    let level = args
        .level
        .as_deref()
        .map(|value| value.to_ascii_lowercase());
    let target = args.target.as_deref().map(str::to_string);
    let contains = args
        .contains
        .as_deref()
        .map(|value| value.to_ascii_lowercase());
    let mut results = runtime_log_events(
        root,
        args.scope,
        args.worktree_id.as_deref(),
        scan_limit(limit),
        |source, line| {
            let event = runtime_log_event_from_line(source, &line);
            matches_runtime_log(
                &event,
                &line,
                level.as_deref(),
                target.as_deref(),
                contains.as_deref(),
            )
            .then_some(event)
        },
    )?;
    results.sort_by(|left, right| runtime_event_sort_desc(right, left));
    results.truncate(limit);
    Ok(results)
}

pub(crate) fn runtime_timeline(
    host: &QueryHost,
    args: RuntimeTimelineArgs,
) -> Result<Vec<RuntimeLogEventView>> {
    let root = workspace_root(host)?;
    let limit = args.limit.unwrap_or(DEFAULT_RUNTIME_TIMELINE_LIMIT);
    if limit == 0 {
        return Ok(Vec::new());
    }

    let contains = args
        .contains
        .as_deref()
        .map(|value| value.to_ascii_lowercase());
    if args.scope.unwrap_or(LogScope::Worktree) == LogScope::Worktree && args.worktree_id.is_none()
    {
        if let Some(state) = read_runtime_state(root)? {
            let mut events = state
                .events
                .into_iter()
                .map(runtime_state_event_view)
                .filter(|event| {
                    contains
                        .as_deref()
                        .is_none_or(|needle| log_contains(event, &event.message, needle))
                })
                .collect::<Vec<_>>();
            if !events.is_empty() {
                if events.len() > limit {
                    events = events.split_off(events.len() - limit);
                }
                return Ok(events);
            }
        }
    }
    let mut events = runtime_log_events(
        root,
        args.scope,
        args.worktree_id.as_deref(),
        scan_limit(limit),
        |source, line| {
            let event = runtime_log_event_from_line(source, &line);
            (is_timeline_event(&event)
                && contains
                    .as_deref()
                    .is_none_or(|needle| log_contains(&event, &line, needle)))
            .then_some(event)
        },
    )?;
    events.sort_by(runtime_event_sort_asc);
    if events.len() > limit {
        events = events.split_off(events.len() - limit);
    }
    Ok(events)
}

fn workspace_root(host: &QueryHost) -> Result<&Path> {
    host.workspace_root()
        .ok_or_else(|| anyhow!("runtime introspection requires a workspace-backed PRISM session"))
}

fn runtime_status_from_inputs(
    inputs: &RuntimeStatusInputs<'_>,
    runtime_state: Option<&RuntimeState>,
    cached_shared_coordination_ref: Option<Option<RuntimeSharedCoordinationRefView>>,
) -> Result<RuntimeStatusView> {
    let paths = RuntimePaths::for_root(inputs.root)?;
    let state_processes = runtime_state
        .map(|state| state.processes.as_slice())
        .unwrap_or(&[]);
    let (processes, process_error) = match list_runtime_processes(inputs.root, state_processes) {
        Ok(processes) => (processes, None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    };
    let daemons = select_kind(&processes, McpProcessKind::Daemon);
    let bridges = select_kind(&processes, McpProcessKind::Bridge);
    let uri = read_uri_file(&paths.uri_file)?
        .or_else(|| daemons.iter().find_map(|process| process.http_uri.clone()));
    let connected_bridge_pids = connected_bridge_ids(&bridges, uri.as_deref());
    let bridge_counts = classify_bridges(&bridges, &connected_bridge_pids);
    let connection =
        daemon_connection_info(inputs.root, &paths, &daemons, process_error.as_deref())?;
    let (freshness, shared_coordination_ref, scopes) =
        match runtime_status_details_from_inputs(inputs, runtime_state, cached_shared_coordination_ref)
        {
            Ok(details) => details,
            Err(error) => {
                let freshness =
                    degraded_runtime_freshness_from_inputs(inputs, runtime_state, &error);
                let scopes = runtime_scopes_from_prism(inputs.prism.as_ref(), &freshness);
                (freshness, None, scopes)
            }
        };

    Ok(RuntimeStatusView {
        root: inputs.root.display().to_string(),
        connection: connection.clone(),
        uri: connection.uri.clone(),
        uri_file: paths.uri_file.display().to_string(),
        log_path: paths.log_path.display().to_string(),
        log_bytes: daemon_log::total_log_bytes(&paths.log_path).ok(),
        mcp_call_log_path: inputs
            .mcp_call_log_store
            .path()
            .map(|path| path.display().to_string()),
        mcp_call_log_bytes: inputs.mcp_call_log_store.file_len(),
        cache_path: paths.cache_path.display().to_string(),
        cache_bytes: file_len(&paths.cache_path),
        health_path: daemon_health_path(&daemons).to_string(),
        health: connection.health.clone(),
        daemon_count: daemons.len(),
        bridge_count: bridges.len(),
        connected_bridge_count: bridge_counts.connected,
        idle_bridge_count: bridge_counts.idle,
        orphan_bridge_count: bridge_counts.orphaned,
        processes: processes
            .into_iter()
            .map(|process| runtime_process_view(process, &connected_bridge_pids))
            .collect(),
        process_error,
        assisted_lease_renewal: runtime_assisted_lease_renewal_view(),
        shared_coordination_ref,
        scopes,
        freshness,
    })
}

fn runtime_status_details_from_inputs(
    inputs: &RuntimeStatusInputs<'_>,
    runtime_state: Option<&RuntimeState>,
    cached_shared_coordination_ref: Option<Option<RuntimeSharedCoordinationRefView>>,
) -> Result<(
    RuntimeFreshnessView,
    Option<RuntimeSharedCoordinationRefView>,
    RuntimeScopesView,
)> {
    let freshness = runtime_freshness_from_inputs(inputs, runtime_state)?;
    let shared_coordination_ref = match cached_shared_coordination_ref {
        Some(value) => value,
        None => shared_coordination_ref_diagnostics(inputs.root)?
            .map(runtime_shared_coordination_ref_view),
    };
    let scopes = runtime_scopes_from_prism(inputs.prism.as_ref(), &freshness);
    Ok((freshness, shared_coordination_ref, scopes))
}

fn runtime_shared_coordination_ref_view(
    value: prism_core::SharedCoordinationRefDiagnostics,
) -> RuntimeSharedCoordinationRefView {
    RuntimeSharedCoordinationRefView {
        ref_name: value.ref_name,
        head_commit: value.head_commit,
        history_depth: value.history_depth,
        max_history_commits: value.max_history_commits,
        snapshot_file_count: value.snapshot_file_count,
        verification_status: value.verification_status,
        authoritative_hydration_allowed: value.authoritative_hydration_allowed,
        degraded: value.degraded,
        verification_error: value.verification_error,
        repair_hint: value.repair_hint,
        current_manifest_digest: value.current_manifest_digest,
        last_verified_manifest_digest: value.last_verified_manifest_digest,
        previous_manifest_digest: value.previous_manifest_digest,
        last_successful_publish_at: value.last_successful_publish_at,
        last_successful_publish_retry_count: value.last_successful_publish_retry_count,
        publish_retry_budget: value.publish_retry_budget,
        compacted_head: value.compacted_head,
        needs_compaction: value.needs_compaction,
        compaction_status: value.compaction_status,
        compaction_mode: value.compaction_mode,
        last_compacted_at: value.last_compacted_at,
        compaction_previous_head_commit: value.compaction_previous_head_commit,
        compaction_previous_history_depth: value.compaction_previous_history_depth,
        archive_boundary_manifest_digest: value.archive_boundary_manifest_digest,
        summary_published_at: value.summary_published_at,
        summary_freshness_status: value.summary_freshness_status,
        authoritative_fallback_required: value.authoritative_fallback_required,
        freshness_reason: value.freshness_reason,
        lagging_task_shard_refs: value.lagging_task_shard_refs,
        lagging_claim_shard_refs: value.lagging_claim_shard_refs,
        lagging_runtime_refs: value.lagging_runtime_refs,
        newest_authoritative_ref_at: value.newest_authoritative_ref_at,
        runtime_descriptor_count: value.runtime_descriptor_count,
        runtime_descriptors: value
            .runtime_descriptors
            .into_iter()
            .map(runtime_shared_coordination_runtime_descriptor_view)
            .collect(),
    }
}

fn runtime_shared_coordination_runtime_descriptor_view(
    value: prism_coordination::RuntimeDescriptor,
) -> RuntimeSharedCoordinationRuntimeDescriptorView {
    RuntimeSharedCoordinationRuntimeDescriptorView {
        runtime_id: value.runtime_id,
        repo_id: value.repo_id,
        worktree_id: value.worktree_id,
        principal_id: value.principal_id,
        instance_started_at: value.instance_started_at,
        last_seen_at: value.last_seen_at,
        branch_ref: value.branch_ref,
        checked_out_commit: value.checked_out_commit,
        capabilities: value
            .capabilities
            .into_iter()
            .map(|capability| match capability {
                prism_coordination::RuntimeDescriptorCapability::CoordinationRefPublisher => {
                    RuntimeDescriptorCapabilityView::CoordinationRefPublisher
                }
                prism_coordination::RuntimeDescriptorCapability::BoundedPeerReads => {
                    RuntimeDescriptorCapabilityView::BoundedPeerReads
                }
                prism_coordination::RuntimeDescriptorCapability::BundleExports => {
                    RuntimeDescriptorCapabilityView::BundleExports
                }
            })
            .collect(),
        discovery_mode: match value.discovery_mode {
            prism_coordination::RuntimeDiscoveryMode::None => RuntimeDiscoveryModeView::None,
            prism_coordination::RuntimeDiscoveryMode::LanDirect => {
                RuntimeDiscoveryModeView::LanDirect
            }
            prism_coordination::RuntimeDiscoveryMode::PublicUrl => {
                RuntimeDiscoveryModeView::PublicUrl
            }
            prism_coordination::RuntimeDiscoveryMode::Full => RuntimeDiscoveryModeView::Full,
        },
        peer_endpoint: value.peer_endpoint,
        public_endpoint: value.public_endpoint,
        peer_transport_identity: value.peer_transport_identity,
        blob_snapshot_head: value.blob_snapshot_head,
        export_policy: value.export_policy,
    }
}

fn runtime_assisted_lease_renewal_view() -> RuntimeAssistedLeaseRenewalView {
    let diagnostics = assisted_lease_renewal_diagnostics();
    RuntimeAssistedLeaseRenewalView {
        enabled: diagnostics.enabled,
        env_var: diagnostics.env_var.to_string(),
        default_enabled: diagnostics.default_enabled,
        authoritative: diagnostics.authoritative,
        scope: diagnostics.scope.to_string(),
        requires_authenticated_mutation: diagnostics.requires_authenticated_mutation,
        bounded_by: diagnostics
            .bounded_by
            .into_iter()
            .map(str::to_string)
            .collect(),
    }
}

fn runtime_freshness_from_inputs(
    inputs: &RuntimeStatusInputs<'_>,
    runtime_state: Option<&RuntimeState>,
) -> Result<RuntimeFreshnessView> {
    let snapshot_revisions = inputs.workspace.snapshot_revisions_for_runtime()?;
    let fs_observed_revision = inputs.workspace.observed_fs_revision();
    let fs_applied_revision = inputs.workspace.applied_fs_revision();
    let fs_dirty = fs_observed_revision != fs_applied_revision;
    let last_refresh = inputs.workspace.last_refresh();
    let workspace_summary = inputs.workspace.workspace_materialization_summary();
    let published_generation = inputs.published_generation.as_ref();
    let queue_snapshot = inputs.queue_snapshot.as_ref();
    let materialization = RuntimeMaterializationView {
        workspace: workspace_materialization_item(
            inputs.loaded_workspace_revision,
            Some(snapshot_revisions.workspace),
            &workspace_summary,
            published_generation
                .and_then(|generation| {
                    generation
                        .domain_states
                        .get(&prism_core::runtime_engine::RuntimeDomain::FileFacts)
                })
                .map(|state| state.materialization),
        ),
        episodic: materialization_item(
            inputs.loaded_episodic_revision,
            Some(snapshot_revisions.episodic),
        ),
        inference: materialization_item(
            inputs.loaded_inference_revision,
            Some(snapshot_revisions.inference),
        ),
        coordination: materialization_item(
            inputs.loaded_coordination_revision,
            Some(snapshot_revisions.coordination),
        ),
    };
    let authoritative_coordination_revision = inputs.workspace.coordination_revision()?;
    let coordination_lag = Some(RuntimeCoordinationLagView {
        authoritative_revision: authoritative_coordination_revision,
        tracked_snapshot: coordination_surface_lag_item(
            "tracked_snapshot",
            optional_coordination_surface_revision(
                inputs.workspace.load_tracked_coordination_snapshot_revision(),
            ),
            authoritative_coordination_revision,
        ),
        startup_checkpoint: coordination_surface_lag_item(
            "startup_checkpoint",
            optional_coordination_surface_revision(
                inputs.workspace.load_coordination_startup_checkpoint_revision(),
            ),
            authoritative_coordination_revision,
        ),
        read_model: coordination_surface_lag_item(
            "read_model",
            optional_coordination_surface_revision(
                inputs
                    .workspace
                    .load_coordination_read_model()
                    .map(|model| model.map(|model| model.revision)),
            ),
            authoritative_coordination_revision,
        ),
        queue_read_model: coordination_surface_lag_item(
            "queue_read_model",
            optional_coordination_surface_revision(
                inputs
                    .workspace
                    .load_coordination_queue_read_model()
                    .map(|model| model.map(|model| model.revision)),
            ),
            authoritative_coordination_revision,
        ),
    });
    let last_build = latest_runtime_event(runtime_state, "built prism-mcp workspace server");
    let last_ready = latest_runtime_event(runtime_state, "prism-mcp daemon ready");

    Ok(RuntimeFreshnessView {
        fs_observed_revision,
        fs_applied_revision,
        fs_dirty,
        generation_id: published_generation
            .as_ref()
            .map(|generation| generation.id.0),
        parent_generation_id: published_generation
            .as_ref()
            .and_then(|generation| generation.parent_id.map(|id| id.0)),
        committed_delta_sequence: published_generation
            .as_ref()
            .and_then(|generation| generation.committed_delta.map(|sequence| sequence.0)),
        last_refresh_path: last_refresh.as_ref().map(|refresh| refresh.path.clone()),
        last_refresh_timestamp: last_refresh
            .as_ref()
            .map(|refresh| refresh.timestamp.clone()),
        last_refresh_duration_ms: last_refresh.as_ref().map(|refresh| refresh.duration_ms),
        last_refresh_loaded_bytes: last_refresh.as_ref().map(|refresh| refresh.loaded_bytes),
        last_refresh_replay_volume: last_refresh.as_ref().map(|refresh| refresh.replay_volume),
        last_refresh_full_rebuild_count: last_refresh
            .as_ref()
            .map(|refresh| refresh.full_rebuild_count),
        last_refresh_workspace_reloaded: last_refresh
            .as_ref()
            .map(|refresh| refresh.workspace_reloaded),
        last_workspace_build_ms: event_field_u64(last_build, "buildMs"),
        last_daemon_ready_ms: event_field_u64(last_ready, "startupMs"),
        materialization: materialization.clone(),
        coordination_lag,
        domains: published_generation
            .as_ref()
            .map(|generation| runtime_domain_views(generation))
            .unwrap_or_default(),
        active_command: queue_snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .active
                .as_ref()
                .map(|command| runtime_command_label(command.kind.clone()).to_string())
        }),
        active_queue_class: queue_snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .active
                .as_ref()
                .map(|command| runtime_queue_class_label(command.queue_class).to_string())
        }),
        queue_depth: queue_snapshot
            .as_ref()
            .map(|snapshot| snapshot.total_depth)
            .unwrap_or(0),
        queued_by_class: queue_snapshot
            .as_ref()
            .map(|snapshot| runtime_queue_depth_views(snapshot))
            .unwrap_or_default(),
        status: freshness_status(
            fs_dirty,
            &materialization,
            last_refresh.as_ref().map(|refresh| refresh.path.as_str()),
        )
        .to_string(),
        error: None,
    })
}

fn degraded_runtime_freshness_from_inputs(
    inputs: &RuntimeStatusInputs<'_>,
    runtime_state: Option<&RuntimeState>,
    error: &anyhow::Error,
) -> RuntimeFreshnessView {
    let snapshot_revisions = inputs.workspace.snapshot_revisions_for_runtime().ok();
    let fs_observed_revision = inputs.workspace.observed_fs_revision();
    let fs_applied_revision = inputs.workspace.applied_fs_revision();
    let fs_dirty = fs_observed_revision != fs_applied_revision;
    let last_refresh = inputs.workspace.last_refresh();
    let workspace_summary = inputs.workspace.workspace_materialization_summary();
    let published_generation = inputs.published_generation.as_ref();
    let queue_snapshot = inputs.queue_snapshot.as_ref();
    let materialization = RuntimeMaterializationView {
        workspace: workspace_materialization_item(
            inputs.loaded_workspace_revision,
            snapshot_revisions.map(|revisions| revisions.workspace),
            &workspace_summary,
            published_generation
                .and_then(|generation| {
                    generation
                        .domain_states
                        .get(&prism_core::runtime_engine::RuntimeDomain::FileFacts)
                })
                .map(|state| state.materialization),
        ),
        episodic: materialization_item(
            inputs.loaded_episodic_revision,
            snapshot_revisions.map(|revisions| revisions.episodic),
        ),
        inference: materialization_item(
            inputs.loaded_inference_revision,
            snapshot_revisions.map(|revisions| revisions.inference),
        ),
        coordination: materialization_item(
            inputs.loaded_coordination_revision,
            snapshot_revisions.map(|revisions| revisions.coordination),
        ),
    };
    let last_build = latest_runtime_event(runtime_state, "built prism-mcp workspace server");
    let last_ready = latest_runtime_event(runtime_state, "prism-mcp daemon ready");

    RuntimeFreshnessView {
        fs_observed_revision,
        fs_applied_revision,
        fs_dirty,
        generation_id: published_generation
            .as_ref()
            .map(|generation| generation.id.0),
        parent_generation_id: published_generation
            .as_ref()
            .and_then(|generation| generation.parent_id.map(|id| id.0)),
        committed_delta_sequence: published_generation
            .as_ref()
            .and_then(|generation| generation.committed_delta.map(|sequence| sequence.0)),
        last_refresh_path: last_refresh.as_ref().map(|refresh| refresh.path.clone()),
        last_refresh_timestamp: last_refresh
            .as_ref()
            .map(|refresh| refresh.timestamp.clone()),
        last_refresh_duration_ms: last_refresh.as_ref().map(|refresh| refresh.duration_ms),
        last_refresh_loaded_bytes: last_refresh.as_ref().map(|refresh| refresh.loaded_bytes),
        last_refresh_replay_volume: last_refresh.as_ref().map(|refresh| refresh.replay_volume),
        last_refresh_full_rebuild_count: last_refresh
            .as_ref()
            .map(|refresh| refresh.full_rebuild_count),
        last_refresh_workspace_reloaded: last_refresh
            .as_ref()
            .map(|refresh| refresh.workspace_reloaded),
        last_workspace_build_ms: event_field_u64(last_build, "buildMs"),
        last_daemon_ready_ms: event_field_u64(last_ready, "startupMs"),
        materialization,
        coordination_lag: None,
        domains: published_generation
            .as_ref()
            .map(|generation| runtime_domain_views(generation))
            .unwrap_or_default(),
        active_command: queue_snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .active
                .as_ref()
                .map(|command| runtime_command_label(command.kind.clone()).to_string())
        }),
        active_queue_class: queue_snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .active
                .as_ref()
                .map(|command| runtime_queue_class_label(command.queue_class).to_string())
        }),
        queue_depth: queue_snapshot
            .as_ref()
            .map(|snapshot| snapshot.total_depth)
            .unwrap_or(0),
        queued_by_class: queue_snapshot
            .as_ref()
            .map(|snapshot| runtime_queue_depth_views(snapshot))
            .unwrap_or_default(),
        status: "degraded".to_string(),
        error: Some(error.to_string()),
    }
}

fn runtime_queue_depth_views(
    snapshot: &prism_core::runtime_engine::WorkspaceRuntimeQueueSnapshot,
) -> Vec<RuntimeQueueDepthView> {
    snapshot
        .queued
        .iter()
        .map(|depth| RuntimeQueueDepthView {
            queue_class: runtime_queue_class_label(depth.queue_class).to_string(),
            depth: depth.depth,
        })
        .collect()
}

fn runtime_domain_views(
    generation: &prism_core::runtime_engine::WorkspacePublishedGeneration,
) -> Vec<RuntimeDomainFreshnessView> {
    generation
        .domain_states
        .iter()
        .map(|(domain, state)| RuntimeDomainFreshnessView {
            domain: runtime_domain_label(*domain).to_string(),
            freshness: runtime_freshness_label(state.freshness).to_string(),
            materialization_depth: runtime_materialization_depth_label(state.materialization)
                .to_string(),
        })
        .collect()
}

fn runtime_domain_label(domain: prism_core::runtime_engine::RuntimeDomain) -> &'static str {
    match domain {
        prism_core::runtime_engine::RuntimeDomain::FileFacts => "file_facts",
        prism_core::runtime_engine::RuntimeDomain::CrossFileEdges => "cross_file_edges",
        prism_core::runtime_engine::RuntimeDomain::Projections => "projections",
        prism_core::runtime_engine::RuntimeDomain::MemoryReanchor => "memory_reanchor",
        prism_core::runtime_engine::RuntimeDomain::Checkpoint => "checkpoint",
        prism_core::runtime_engine::RuntimeDomain::Coordination => "coordination",
    }
}

fn runtime_freshness_label(state: RuntimeFreshnessState) -> &'static str {
    match state {
        RuntimeFreshnessState::Current => "current",
        RuntimeFreshnessState::Pending => "pending",
        RuntimeFreshnessState::Stale => "stale",
        RuntimeFreshnessState::Recovery => "recovery",
    }
}

fn runtime_materialization_depth_label(depth: RuntimeMaterializationDepth) -> &'static str {
    match depth {
        RuntimeMaterializationDepth::Shallow => "shallow",
        RuntimeMaterializationDepth::Medium => "medium",
        RuntimeMaterializationDepth::Deep => "deep",
        RuntimeMaterializationDepth::KnownUnmaterialized => "known_unmaterialized",
        RuntimeMaterializationDepth::OutOfScope => "out_of_scope",
    }
}

fn runtime_command_label(
    kind: prism_core::runtime_engine::WorkspaceRuntimeCommandKind,
) -> &'static str {
    match kind {
        prism_core::runtime_engine::WorkspaceRuntimeCommandKind::InteractiveMutation => {
            "interactive_mutation"
        }
        prism_core::runtime_engine::WorkspaceRuntimeCommandKind::FollowUpMutation => {
            "follow_up_mutation"
        }
        prism_core::runtime_engine::WorkspaceRuntimeCommandKind::PreparePaths => "prepare_paths",
        prism_core::runtime_engine::WorkspaceRuntimeCommandKind::ApplyPreparedDelta => {
            "apply_prepared_delta"
        }
        prism_core::runtime_engine::WorkspaceRuntimeCommandKind::SettleDomain(_) => "settle_domain",
        prism_core::runtime_engine::WorkspaceRuntimeCommandKind::MaterializeCheckpoint => {
            "materialize_checkpoint"
        }
    }
}

fn runtime_queue_class_label(
    queue_class: prism_core::runtime_engine::WorkspaceRuntimeQueueClass,
) -> &'static str {
    match queue_class {
        prism_core::runtime_engine::WorkspaceRuntimeQueueClass::InteractiveMutation => {
            "interactive_mutation"
        }
        prism_core::runtime_engine::WorkspaceRuntimeQueueClass::FollowUpMutation => {
            "follow_up_mutation"
        }
        prism_core::runtime_engine::WorkspaceRuntimeQueueClass::FastPrepare => "fast_prepare",
        prism_core::runtime_engine::WorkspaceRuntimeQueueClass::Settle => "settle",
        prism_core::runtime_engine::WorkspaceRuntimeQueueClass::CheckpointMaterialization => {
            "checkpoint_materialization"
        }
    }
}

fn runtime_scopes_from_prism(
    prism: &prism_query::Prism,
    freshness: &RuntimeFreshnessView,
) -> RuntimeScopesView {
    let projections = runtime_projection_scopes(prism, freshness)
        .into_iter()
        .map(projection_scope_view)
        .collect();
    let coordination = prism.coordination_snapshot();
    let overlays = overlay_scope_views(&coordination);

    RuntimeScopesView {
        projections,
        overlays,
    }
}

fn projection_scope_view(scope: ProjectionScopeReadModel) -> RuntimeProjectionScopeView {
    RuntimeProjectionScopeView {
        scope: scope.scope,
        projection_class: projection_class_view(scope.projection_class),
        authority_planes: scope
            .authority_planes
            .into_iter()
            .map(projection_authority_plane_view)
            .collect(),
        freshness: projection_freshness_state_view(scope.freshness),
        materialization: projection_materialization_state_view(scope.materialization),
        concept_count: scope.concept_count,
        relation_count: scope.relation_count,
        contract_count: scope.contract_count,
        co_change_lineage_count: scope.co_change_lineage_count,
        validation_lineage_count: scope.validation_lineage_count,
        read_models: scope
            .read_models
            .into_iter()
            .map(projection_read_model_view)
            .collect(),
    }
}

fn overlay_scope_views(
    snapshot: &prism_coordination::CoordinationSnapshot,
) -> Vec<RuntimeOverlayScopeView> {
    let canonical_snapshot = snapshot.to_canonical_snapshot_v2();
    vec![
        RuntimeOverlayScopeView {
            scope: "repo".to_string(),
            plan_count: canonical_snapshot.plans.len(),
            plan_node_count: canonical_snapshot.tasks.len(),
            overlay_count: 0,
        },
        RuntimeOverlayScopeView {
            scope: "worktree".to_string(),
            plan_count: 0,
            plan_node_count: 0,
            overlay_count: snapshot
                .tasks
                .iter()
                .filter(|task| task.worktree_id.is_some() || task.branch_ref.is_some())
                .count(),
        },
        RuntimeOverlayScopeView {
            scope: "session".to_string(),
            plan_count: 0,
            plan_node_count: 0,
            overlay_count: snapshot
                .tasks
                .iter()
                .filter(|task| task.session.is_some())
                .count(),
        },
    ]
}

fn projection_read_model_view(model: ProjectionReadModel) -> ProjectionReadModelView {
    ProjectionReadModelView {
        name: model.name,
        projection_class: projection_class_view(model.projection_class),
        authority_planes: model
            .authority_planes
            .into_iter()
            .map(projection_authority_plane_view)
            .collect(),
        freshness: projection_freshness_state_view(model.freshness),
        materialization: projection_materialization_state_view(model.materialization),
        entry_count: model.entry_count,
    }
}

fn projection_class_view(class: ProjectionClass) -> ProjectionClassView {
    match class {
        ProjectionClass::Published => ProjectionClassView::Published,
        ProjectionClass::Serving => ProjectionClassView::Serving,
        ProjectionClass::AdHoc => ProjectionClassView::AdHoc,
    }
}

fn projection_authority_plane_view(
    plane: ProjectionAuthorityPlane,
) -> ProjectionAuthorityPlaneView {
    match plane {
        ProjectionAuthorityPlane::PublishedRepo => ProjectionAuthorityPlaneView::PublishedRepo,
        ProjectionAuthorityPlane::SharedRuntime => ProjectionAuthorityPlaneView::SharedRuntime,
    }
}

fn projection_freshness_state_view(
    state: ProjectionFreshnessState,
) -> ProjectionFreshnessStateView {
    match state {
        ProjectionFreshnessState::Current => ProjectionFreshnessStateView::Current,
        ProjectionFreshnessState::Pending => ProjectionFreshnessStateView::Pending,
        ProjectionFreshnessState::Stale => ProjectionFreshnessStateView::Stale,
        ProjectionFreshnessState::Recovery => ProjectionFreshnessStateView::Recovery,
        ProjectionFreshnessState::Deferred => ProjectionFreshnessStateView::Deferred,
        ProjectionFreshnessState::Unknown => ProjectionFreshnessStateView::Unknown,
    }
}

fn projection_materialization_state_view(
    state: ProjectionMaterializationState,
) -> ProjectionMaterializationStateView {
    match state {
        ProjectionMaterializationState::Materialized => {
            ProjectionMaterializationStateView::Materialized
        }
        ProjectionMaterializationState::Partial => ProjectionMaterializationStateView::Partial,
        ProjectionMaterializationState::Deferred => ProjectionMaterializationStateView::Deferred,
        ProjectionMaterializationState::KnownUnmaterialized => {
            ProjectionMaterializationStateView::KnownUnmaterialized
        }
    }
}

fn materialization_item(
    loaded_revision: u64,
    current_revision: Option<u64>,
) -> RuntimeMaterializationItemView {
    RuntimeMaterializationItemView {
        status: materialization_status(loaded_revision, current_revision).to_string(),
        depth: materialization_depth(loaded_revision, current_revision).to_string(),
        loaded_revision,
        current_revision,
        coverage: None,
        boundaries: Vec::new(),
    }
}

fn coordination_surface_lag_item(
    name: &str,
    revision: Option<u64>,
    authoritative_revision: u64,
) -> RuntimeCoordinationSurfaceLagItemView {
    RuntimeCoordinationSurfaceLagItemView {
        name: name.to_string(),
        status: coordination_surface_lag_status(revision, authoritative_revision).to_string(),
        revision,
        authoritative_revision,
    }
}

fn optional_coordination_surface_revision(result: Result<Option<u64>>) -> Option<u64> {
    result.ok().flatten()
}

fn workspace_materialization_item(
    loaded_revision: u64,
    current_revision: Option<u64>,
    summary: &prism_core::WorkspaceMaterializationSummary,
    materialization_depth: Option<RuntimeMaterializationDepth>,
) -> RuntimeMaterializationItemView {
    RuntimeMaterializationItemView {
        status: materialization_status(loaded_revision, current_revision).to_string(),
        depth: materialization_depth
            .map(runtime_materialization_depth_label)
            .unwrap_or_else(|| summary.depth())
            .to_string(),
        loaded_revision,
        current_revision,
        coverage: Some(RuntimeMaterializationCoverageView {
            known_files: summary.known_files,
            known_directories: summary.known_directories,
            materialized_files: summary.materialized_files,
            materialized_nodes: summary.materialized_nodes,
            materialized_edges: summary.materialized_edges,
        }),
        boundaries: summary
            .boundaries
            .iter()
            .map(|boundary| RuntimeBoundaryRegionView {
                id: boundary.id.clone(),
                path: boundary.path.display().to_string(),
                provenance: boundary.provenance.clone(),
                materialization_state: boundary.materialization_state.clone(),
                scope_state: boundary.scope_state.clone(),
                known_file_count: boundary.known_file_count,
                materialized_file_count: boundary.materialized_file_count,
            })
            .collect(),
    }
}

fn materialization_status(loaded_revision: u64, current_revision: Option<u64>) -> &'static str {
    match current_revision {
        Some(current_revision) if loaded_revision == current_revision => "current",
        Some(_) => "stale",
        None => "unknown",
    }
}

fn materialization_depth(loaded_revision: u64, current_revision: Option<u64>) -> &'static str {
    if loaded_revision == 0 && current_revision.unwrap_or(0) == 0 {
        "shallow"
    } else {
        "medium"
    }
}

fn coordination_surface_lag_status(
    revision: Option<u64>,
    authoritative_revision: u64,
) -> &'static str {
    match revision {
        Some(revision) if revision == authoritative_revision => "current",
        Some(_) => "stale",
        None => "missing",
    }
}

fn freshness_status(
    fs_dirty: bool,
    materialization: &RuntimeMaterializationView,
    last_refresh_path: Option<&str>,
) -> &'static str {
    if fs_dirty {
        return "refresh-queued";
    }
    if last_refresh_path == Some("deferred") {
        return "deferred";
    }
    let statuses = [
        materialization.workspace.status.as_str(),
        materialization.episodic.status.as_str(),
        materialization.inference.status.as_str(),
        materialization.coordination.status.as_str(),
    ];
    if statuses.contains(&"stale") {
        "stale"
    } else if statuses.contains(&"unknown") {
        "unknown"
    } else {
        "current"
    }
}

fn latest_runtime_event<'a>(
    runtime_state: Option<&'a RuntimeState>,
    message: &str,
) -> Option<&'a RuntimeEventRecord> {
    runtime_state?
        .events
        .iter()
        .rev()
        .find(|event| event.message == message)
}

fn event_field_u64(event: Option<&RuntimeEventRecord>, key: &str) -> Option<u64> {
    let value = event?.fields.get(key)?;
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|value| value.parse::<u64>().ok()))
}

fn file_len(path: &Path) -> Option<u64> {
    fs::metadata(path).ok().map(|metadata| metadata.len())
}

fn runtime_process_view(
    process: McpProcess,
    connected_bridge_pids: &BTreeSet<u32>,
) -> RuntimeProcessView {
    let bridge_state = bridge_state(&process, connected_bridge_pids).map(bridge_state_label);
    RuntimeProcessView {
        pid: process.pid,
        parent_pid: process.ppid,
        rss_kb: process.rss_kb,
        rss_mb: process.rss_kb as f64 / 1024.0,
        elapsed: process.elapsed,
        kind: match process.kind {
            McpProcessKind::Daemon => "daemon",
            McpProcessKind::Bridge => "bridge",
        }
        .to_string(),
        command: process.command,
        health_path: process.health_path,
        bridge_state,
    }
}

fn runtime_state_event_view(event: RuntimeEventRecord) -> RuntimeLogEventView {
    RuntimeLogEventView {
        timestamp: Some(event.timestamp),
        level: Some(event.level),
        message: event.message,
        target: Some(event.target),
        file: event.file,
        line_number: event.line_number,
        repo_id: None,
        worktree_id: None,
        workspace_root: None,
        log_path: None,
        fields: value_object_fields(event.fields),
    }
}

fn latest_runtime_state_event_view(
    runtime_state: Option<&RuntimeState>,
) -> Option<RuntimeLogEventView> {
    runtime_state
        .and_then(|state| state.events.last().cloned())
        .map(runtime_state_event_view)
}

fn health_status(
    uri: &Option<String>,
    daemons: &[McpProcess],
    process_error: Option<&str>,
) -> RuntimeHealthView {
    if let Some(error) = process_error {
        return RuntimeHealthView {
            ok: false,
            detail: format!("process listing failed: {error}"),
        };
    }

    let Some(uri) = uri else {
        return RuntimeHealthView {
            ok: false,
            detail: "missing uri file".to_string(),
        };
    };

    if daemons.is_empty() {
        return RuntimeHealthView {
            ok: true,
            detail: format!("ok ({uri}; uri file present, no live daemon process record)"),
        };
    }

    let detail = if daemons.len() == 1 {
        format!("ok ({uri})")
    } else {
        format!("ok ({uri}; {} daemon processes)", daemons.len())
    };
    RuntimeHealthView { ok: true, detail }
}

fn list_runtime_processes(
    root: &Path,
    state_processes: &[RuntimeProcessRecord],
) -> Result<Vec<McpProcess>> {
    if state_processes.is_empty() {
        return Ok(Vec::new());
    }
    runtime_state_processes(root, state_processes)
}

fn runtime_state_processes(
    root: &Path,
    state_processes: &[RuntimeProcessRecord],
) -> Result<Vec<McpProcess>> {
    let snapshots = list_process_snapshots()?;
    Ok(state_processes
        .iter()
        .filter(|record| process_is_live(record.pid))
        .filter_map(|record| runtime_process_from_record(root, record, &snapshots))
        .collect())
}

fn runtime_process_from_record(
    root: &Path,
    record: &RuntimeProcessRecord,
    snapshots: &[ProcessSnapshot],
) -> Option<McpProcess> {
    let kind = match record.kind.as_str() {
        "daemon" => McpProcessKind::Daemon,
        "bridge" => McpProcessKind::Bridge,
        _ => return None,
    };
    let snapshot = snapshots.iter().find(|snapshot| snapshot.pid == record.pid);
    Some(McpProcess {
        pid: record.pid,
        ppid: snapshot.map(|snapshot| snapshot.ppid).unwrap_or(0),
        rss_kb: snapshot.map(|snapshot| snapshot.rss_kb).unwrap_or(0),
        elapsed: snapshot
            .map(|snapshot| snapshot.elapsed.clone())
            .unwrap_or_else(|| elapsed_since(record.started_at)),
        command: snapshot
            .map(|snapshot| snapshot.command.clone())
            .unwrap_or_else(|| {
                format!("prism-mcp --mode {} --root {}", record.kind, root.display())
            }),
        kind,
        health_path: record.health_path.clone(),
        http_uri: record.http_uri.clone(),
        upstream_uri: record.upstream_uri.clone(),
    })
}

fn list_process_snapshots() -> Result<Vec<ProcessSnapshot>> {
    let output = Command::new("ps")
        .args(["-axo", "pid=,ppid=,rss=,etime=,command="])
        .output()
        .context("failed to list processes with ps")?;
    if !output.status.success() {
        return Err(anyhow!(
            "ps failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_process_snapshot)
        .collect())
}

fn parse_process_snapshot(line: &str) -> Option<ProcessSnapshot> {
    let mut parts = line.split_whitespace();
    let pid = parts.next()?.parse().ok()?;
    let ppid = parts.next()?.parse::<u32>().ok()?;
    let rss_kb = parts.next()?.parse().ok()?;
    let elapsed = parts.next()?.to_string();
    let command = parts.collect::<Vec<_>>().join(" ");
    if command.is_empty() {
        return None;
    }
    Some(ProcessSnapshot {
        pid,
        ppid,
        rss_kb,
        elapsed,
        command,
    })
}

fn bridge_state(
    process: &McpProcess,
    connected_bridge_pids: &BTreeSet<u32>,
) -> Option<BridgeState> {
    if process.kind != McpProcessKind::Bridge {
        return None;
    }
    if connected_bridge_pids.contains(&process.pid) {
        Some(BridgeState::Connected)
    } else if process.ppid == 1 {
        Some(BridgeState::Orphaned)
    } else {
        Some(BridgeState::Idle)
    }
}

fn daemon_connection_info(
    root: &Path,
    paths: &RuntimePaths,
    daemons: &[McpProcess],
    process_error: Option<&str>,
) -> Result<ConnectionInfoView> {
    let uri = read_uri_file(&paths.uri_file)?
        .or_else(|| daemons.iter().find_map(|process| process.http_uri.clone()));
    let health_path = daemon_health_path(daemons).to_string();
    let health_uri = uri.as_ref().map(|uri| join_health_uri(uri, &health_path));
    let health = health_status(&uri, daemons, process_error);
    Ok(ConnectionInfoView {
        root: root.display().to_string(),
        mode: "direct-daemon".to_string(),
        transport: "streamable-http".to_string(),
        uri,
        uri_file: paths.uri_file.display().to_string(),
        health_uri,
        health,
        bridge_role: "stdio-compatibility-only".to_string(),
    })
}

fn join_health_uri(uri: &str, health_path: &str) -> String {
    let base = uri
        .split_once("://")
        .map(|(scheme, rest)| {
            let authority = rest.split('/').next().unwrap_or(rest);
            format!("{scheme}://{authority}")
        })
        .unwrap_or_else(|| uri.to_string());
    format!(
        "{}{}",
        base.trim_end_matches('/'),
        normalize_route_path(health_path)
    )
}

fn normalize_route_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        DEFAULT_HEALTH_PATH.to_string()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

fn bridge_state_label(state: BridgeState) -> String {
    match state {
        BridgeState::Connected => "connected",
        BridgeState::Idle => "idle",
        BridgeState::Orphaned => "orphaned",
    }
    .to_string()
}

fn classify_bridges(bridges: &[McpProcess], connected_bridge_pids: &BTreeSet<u32>) -> BridgeCounts {
    let mut counts = BridgeCounts::default();
    for process in bridges {
        match bridge_state(process, connected_bridge_pids) {
            Some(BridgeState::Connected) => counts.connected += 1,
            Some(BridgeState::Idle) => counts.idle += 1,
            Some(BridgeState::Orphaned) => counts.orphaned += 1,
            None => {}
        }
    }
    counts
}

fn connected_bridge_ids(bridges: &[McpProcess], uri: Option<&str>) -> BTreeSet<u32> {
    if let Some(uri) = uri {
        if let Ok(connected) = connected_process_ids_for_uri(uri) {
            return connected;
        }
    }

    bridges
        .iter()
        .filter(|bridge| {
            bridge
                .upstream_uri
                .as_deref()
                .zip(uri)
                .map(|(upstream, candidate)| upstream == candidate)
                .unwrap_or_else(|| bridge.upstream_uri.is_some())
        })
        .map(|bridge| bridge.pid)
        .collect()
}

fn connected_process_ids_for_uri(uri: &str) -> Result<BTreeSet<u32>> {
    let Some(port) = uri_port(uri) else {
        return Ok(BTreeSet::new());
    };
    let output = Command::new("lsof")
        .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:ESTABLISHED", "-Fp"])
        .output()
        .context("failed to inspect established TCP connections with lsof")?;
    if !output.status.success() {
        return Ok(BTreeSet::new());
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix('p'))
        .filter_map(|pid| pid.parse::<u32>().ok())
        .collect())
}

fn uri_port(uri: &str) -> Option<u16> {
    uri.strip_prefix("http://")
        .or_else(|| uri.strip_prefix("https://"))
        .and_then(|rest| rest.split('/').next())
        .and_then(|authority| authority.rsplit_once(':'))
        .and_then(|(_, port)| port.parse::<u16>().ok())
}

fn select_kind(processes: &[McpProcess], kind: McpProcessKind) -> Vec<McpProcess> {
    processes
        .iter()
        .filter(|process| process.kind == kind)
        .cloned()
        .collect()
}

fn daemon_health_path(daemons: &[McpProcess]) -> &str {
    daemons
        .first()
        .and_then(|daemon| daemon.health_path.as_deref())
        .unwrap_or(DEFAULT_HEALTH_PATH)
}

fn elapsed_since(started_at: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(started_at);
    let elapsed = now.saturating_sub(started_at);
    let hours = elapsed / 3600;
    let minutes = (elapsed % 3600) / 60;
    let seconds = elapsed % 60;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

fn read_uri_file(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let value = fs::read_to_string(path)
        .with_context(|| format!("failed to read URI file {}", path.display()))?;
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    Ok(Some(value.to_string()))
}

fn scan_limit(limit: usize) -> usize {
    limit
        .saturating_mul(8)
        .max(DEFAULT_LOG_SCAN_LINES)
        .min(MAX_LOG_SCAN_LINES)
}

fn parse_log_event(line: &str) -> RuntimeLogEventView {
    match serde_json::from_str::<DaemonLogRecord>(line) {
        Ok(record) => runtime_log_event_view(record),
        Err(_) => RuntimeLogEventView {
            timestamp: None,
            level: None,
            message: line.to_string(),
            target: None,
            file: None,
            line_number: None,
            repo_id: None,
            worktree_id: None,
            workspace_root: None,
            log_path: None,
            fields: None,
        },
    }
}

fn runtime_log_event_view(record: DaemonLogRecord) -> RuntimeLogEventView {
    RuntimeLogEventView {
        timestamp: record.timestamp,
        level: record.level,
        message: record
            .message
            .unwrap_or_else(|| "<missing message>".to_string()),
        target: record.target,
        file: record.filename,
        line_number: record.line_number,
        repo_id: None,
        worktree_id: None,
        workspace_root: None,
        log_path: None,
        fields: (!record.extra.is_empty()).then_some(Value::Object(record.extra)),
    }
}

fn runtime_log_events(
    root: &Path,
    scope: Option<LogScope>,
    worktree_id: Option<&str>,
    line_limit: usize,
    include: impl Fn(&RepoLogSource, String) -> Option<RuntimeLogEventView>,
) -> Result<Vec<RuntimeLogEventView>> {
    let sources = select_log_sources(root, scope, worktree_id)?;
    let mut events = Vec::new();
    for source in sources {
        for line in daemon_log::tail_lines(&source.daemon_log_path, line_limit)? {
            if let Some(event) = include(&source, line) {
                events.push(event);
            }
        }
    }
    Ok(events)
}

fn runtime_log_event_from_line(source: &RepoLogSource, line: &str) -> RuntimeLogEventView {
    let mut event = parse_log_event(line);
    event.repo_id = Some(source.repo_id.clone());
    event.worktree_id = Some(source.worktree_id.clone());
    event.workspace_root = Some(source.workspace_root.clone());
    event.log_path = Some(source.daemon_log_path.display().to_string());
    event
}

fn runtime_event_sort_desc(
    left: &RuntimeLogEventView,
    right: &RuntimeLogEventView,
) -> std::cmp::Ordering {
    left.timestamp
        .cmp(&right.timestamp)
        .then_with(|| left.log_path.cmp(&right.log_path))
        .then_with(|| left.message.cmp(&right.message))
}

fn runtime_event_sort_asc(
    left: &RuntimeLogEventView,
    right: &RuntimeLogEventView,
) -> std::cmp::Ordering {
    left.timestamp
        .cmp(&right.timestamp)
        .then_with(|| left.log_path.cmp(&right.log_path))
        .then_with(|| left.message.cmp(&right.message))
}

fn value_object_fields(value: Value) -> Option<Value> {
    match value {
        Value::Null => None,
        Value::Object(map) if map.is_empty() => None,
        other => Some(other),
    }
}

fn matches_runtime_log(
    event: &RuntimeLogEventView,
    line: &str,
    level: Option<&str>,
    target: Option<&str>,
    contains: Option<&str>,
) -> bool {
    if level.is_some_and(|expected| {
        event
            .level
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref()
            != Some(expected)
    }) {
        return false;
    }
    if target.is_some_and(|expected| event.target.as_deref() != Some(expected)) {
        return false;
    }
    if contains.is_some_and(|needle| !log_contains(event, line, needle)) {
        return false;
    }
    true
}

fn log_contains(event: &RuntimeLogEventView, line: &str, needle: &str) -> bool {
    if line.to_ascii_lowercase().contains(needle) {
        return true;
    }
    event
        .fields
        .as_ref()
        .map(Value::to_string)
        .unwrap_or_default()
        .to_ascii_lowercase()
        .contains(needle)
}

fn is_timeline_event(event: &RuntimeLogEventView) -> bool {
    let message = event.message.as_str();
    matches!(
        message,
        "starting prism-mcp"
            | "building prism-mcp workspace server"
            | "opened prism sqlite store"
            | "loaded prism graph snapshot"
            | "loaded prism projection snapshot"
            | "prepared prism workspace indexer"
            | "starting prism workspace indexing"
            | "collected prism pending file parses"
            | "finished prism parse and update loop"
            | "finished prism missing-file removal phase"
            | "finished prism edge resolution phase"
            | "skipped prism index persistence batch because workspace state is unchanged"
            | "reanchored persisted prism memory"
            | "completed prism workspace indexing"
            | "built prism query state"
            | "built prism workspace session"
            | "built prism-mcp workspace server"
            | "prism-mcp daemon ready"
            | "prism-mcp bridge resolved upstream"
            | "prism-mcp bridge connected"
            | "prism-mcp workspace refresh"
    )
}

impl RuntimePaths {
    fn for_root(root: &Path) -> Result<Self> {
        let prism_paths = PrismPaths::for_workspace_root(root)?;
        Ok(Self {
            uri_file: prism_paths.mcp_http_uri_path()?,
            log_path: prism_paths.mcp_daemon_log_path()?,
            cache_path: prism_paths.worktree_cache_db_path()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::ensure_process_test_prism_home;
    use serde_json::json;

    fn temp_root(label: &str) -> PathBuf {
        let _ = ensure_process_test_prism_home();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "prism-runtime-views-tests-{label}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn runtime_paths_report_worktree_cache_db() {
        let root = temp_root("paths-cache");
        fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();

        let paths = RuntimePaths::for_root(&root).unwrap();
        let prism_paths = PrismPaths::for_workspace_root(&root).unwrap();

        assert_eq!(
            paths.cache_path,
            prism_paths.worktree_cache_db_path().unwrap()
        );
        assert_ne!(
            paths.cache_path,
            prism_paths.shared_runtime_db_path().unwrap()
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parses_json_log_lines_into_runtime_events() {
        let event = parse_log_event(
            r#"{"timestamp":"2026-03-26T15:12:35Z","level":"INFO","message":"starting prism-mcp","target":"prism_mcp::logging","filename":"crates/prism-mcp/src/logging.rs","line_number":53,"mode":"daemon"}"#,
        );

        assert_eq!(event.timestamp.as_deref(), Some("2026-03-26T15:12:35Z"));
        assert_eq!(event.level.as_deref(), Some("INFO"));
        assert_eq!(event.message, "starting prism-mcp");
        assert_eq!(event.target.as_deref(), Some("prism_mcp::logging"));
        assert_eq!(
            event.file.as_deref(),
            Some("crates/prism-mcp/src/logging.rs")
        );
        assert_eq!(event.line_number, Some(53));
        assert_eq!(
            event.fields.as_ref().and_then(|value| value.get("mode")),
            Some(&Value::String("daemon".to_string()))
        );
    }

    #[test]
    fn runtime_timeline_filters_to_startup_and_refresh_events() {
        assert!(is_timeline_event(&RuntimeLogEventView {
            timestamp: Some("2026-03-26T15:12:35Z".to_string()),
            level: Some("INFO".to_string()),
            message: "completed prism workspace indexing".to_string(),
            target: Some("prism_core::indexer".to_string()),
            file: None,
            line_number: None,
            fields: None,
            repo_id: None,
            worktree_id: None,
            workspace_root: None,
            log_path: None,
        }));
        assert!(!is_timeline_event(&RuntimeLogEventView {
            timestamp: Some("2026-03-26T15:12:35Z".to_string()),
            level: Some("WARN".to_string()),
            message: "response error".to_string(),
            target: Some("rmcp::service".to_string()),
            file: None,
            line_number: None,
            fields: None,
            repo_id: None,
            worktree_id: None,
            workspace_root: None,
            log_path: None,
        }));
        assert!(is_timeline_event(&RuntimeLogEventView {
            timestamp: Some("2026-03-26T15:12:36Z".to_string()),
            level: Some("INFO".to_string()),
            message: "prism-mcp bridge resolved upstream".to_string(),
            target: Some("prism_mcp::daemon_mode".to_string()),
            file: None,
            line_number: None,
            fields: Some(json!({
                "resolutionSource": "existing_healthy_daemon",
                "resolutionMs": 12,
                "daemonWaitMs": 0,
            })),
            repo_id: None,
            worktree_id: None,
            workspace_root: None,
            log_path: None,
        }));
    }

    #[test]
    fn health_status_uses_process_state_instead_of_self_http_probe() {
        let root = temp_root("health-status");
        let daemon = McpProcess {
            pid: 29267,
            ppid: 1,
            rss_kb: 4454352,
            elapsed: "02:12:24".to_string(),
            command: format!(
                "{} --mode daemon",
                root.join("target/release/prism-mcp").display()
            ),
            kind: McpProcessKind::Daemon,
            health_path: Some("/healthz".to_string()),
            http_uri: Some("http://127.0.0.1:52695/mcp".to_string()),
            upstream_uri: None,
        };

        let healthy = health_status(
            &Some("http://127.0.0.1:52695/mcp".to_string()),
            &[daemon.clone()],
            None,
        );
        assert!(healthy.ok);
        assert_eq!(healthy.detail, "ok (http://127.0.0.1:52695/mcp)");

        let missing_daemon = health_status(&Some("http://127.0.0.1:9/mcp".to_string()), &[], None);
        assert!(missing_daemon.ok);
        assert_eq!(
            missing_daemon.detail,
            "ok (http://127.0.0.1:9/mcp; uri file present, no live daemon process record)"
        );

        let process_error = health_status(
            &Some("http://127.0.0.1:52695/mcp".to_string()),
            &[daemon],
            Some("ps failed"),
        );
        assert!(!process_error.ok);
        assert_eq!(process_error.detail, "process listing failed: ps failed");
    }

    #[test]
    fn classify_bridges_distinguishes_connected_idle_and_orphaned() {
        let bridges = vec![
            McpProcess {
                pid: 10,
                ppid: 1000,
                rss_kb: 1,
                elapsed: "00:01".to_string(),
                command: "prism-mcp --mode bridge".to_string(),
                kind: McpProcessKind::Bridge,
                health_path: None,
                http_uri: None,
                upstream_uri: Some("http://127.0.0.1:52695/mcp".to_string()),
            },
            McpProcess {
                pid: 11,
                ppid: 1001,
                rss_kb: 1,
                elapsed: "00:02".to_string(),
                command: "prism-mcp --mode bridge".to_string(),
                kind: McpProcessKind::Bridge,
                health_path: None,
                http_uri: None,
                upstream_uri: None,
            },
            McpProcess {
                pid: 12,
                ppid: 1002,
                rss_kb: 1,
                elapsed: "02:01".to_string(),
                command: "prism-mcp --mode bridge".to_string(),
                kind: McpProcessKind::Bridge,
                health_path: None,
                http_uri: None,
                upstream_uri: None,
            },
            McpProcess {
                pid: 13,
                ppid: 1,
                rss_kb: 1,
                elapsed: "00:03".to_string(),
                command: "prism-mcp --mode bridge".to_string(),
                kind: McpProcessKind::Bridge,
                health_path: None,
                http_uri: None,
                upstream_uri: None,
            },
        ];
        let connected = BTreeSet::from([10_u32]);

        let counts = classify_bridges(&bridges, &connected);

        assert_eq!(counts.connected, 1);
        assert_eq!(counts.idle, 2);
        assert_eq!(counts.orphaned, 1);
        assert_eq!(
            bridge_state(&bridges[0], &connected).map(bridge_state_label),
            Some("connected".to_string())
        );
        assert_eq!(
            bridge_state(&bridges[1], &connected).map(bridge_state_label),
            Some("idle".to_string())
        );
        assert_eq!(
            bridge_state(&bridges[2], &connected).map(bridge_state_label),
            Some("idle".to_string())
        );
        assert_eq!(
            bridge_state(&bridges[3], &connected).map(bridge_state_label),
            Some("orphaned".to_string())
        );
    }

    #[test]
    fn connected_bridge_ids_prefer_live_socket_ownership_over_upstream_history() {
        let bridges = vec![McpProcess {
            pid: 10,
            ppid: 1000,
            rss_kb: 1,
            elapsed: "00:01".to_string(),
            command: "prism-mcp --mode bridge".to_string(),
            kind: McpProcessKind::Bridge,
            health_path: None,
            http_uri: None,
            upstream_uri: Some("http://127.0.0.1:9/mcp".to_string()),
        }];

        let connected = connected_bridge_ids(&bridges, Some("http://127.0.0.1:9/mcp"));

        assert!(
            connected.is_empty(),
            "historical upstream URIs should not masquerade as live connections"
        );
    }

    #[test]
    fn runtime_state_processes_filter_out_dead_runtime_records() {
        let root = temp_root("runtime-status-test");
        let live = RuntimeProcessRecord {
            pid: std::process::id(),
            kind: "daemon".to_string(),
            started_at: 1,
            health_path: Some("/healthz".to_string()),
            http_uri: Some("http://127.0.0.1:52695/mcp".to_string()),
            upstream_uri: None,
            restart_nonce: Some("live".to_string()),
        };
        let dead = RuntimeProcessRecord {
            pid: 999_999,
            kind: "daemon".to_string(),
            started_at: 1,
            health_path: Some("/healthz".to_string()),
            http_uri: Some("http://127.0.0.1:42695/mcp".to_string()),
            upstream_uri: None,
            restart_nonce: Some("dead".to_string()),
        };

        let processes = runtime_state_processes(&root, &[live, dead]).unwrap();
        assert_eq!(processes.len(), 1);
        assert_eq!(processes[0].pid, std::process::id());
    }

    #[test]
    fn runtime_process_from_record_prefers_live_process_snapshot_metadata() {
        let root = temp_root("runtime-process-from-record");
        let record = RuntimeProcessRecord {
            pid: 42,
            kind: "daemon".to_string(),
            started_at: 1,
            health_path: Some("/healthz".to_string()),
            http_uri: Some("http://127.0.0.1:52695/mcp".to_string()),
            upstream_uri: None,
            restart_nonce: Some("live".to_string()),
        };
        let snapshots = vec![ProcessSnapshot {
            pid: 42,
            ppid: 7,
            rss_kb: 123_456,
            elapsed: "00:12".to_string(),
            command: format!(
                "{} --mode daemon --root {}",
                root.join("bin/prism-mcp").display(),
                root.display()
            ),
        }];

        let process = runtime_process_from_record(&root, &record, &snapshots)
            .expect("runtime process should be built");

        assert_eq!(process.pid, 42);
        assert_eq!(process.ppid, 7);
        assert_eq!(process.rss_kb, 123_456);
        assert_eq!(process.elapsed, "00:12");
        assert!(process.command.contains("--mode daemon"));
        assert_eq!(
            process.http_uri.as_deref(),
            Some("http://127.0.0.1:52695/mcp")
        );
    }

    #[test]
    fn parse_process_snapshot_reads_ps_layout() {
        let root = temp_root("parse-process-snapshot");
        let snapshot = parse_process_snapshot(&format!(
            "33725     1 1006400    05:04 {} --mode daemon --daemonize",
            root.join("target/release/prism-mcp").display()
        ))
        .expect("ps snapshot should parse");

        assert_eq!(snapshot.pid, 33725);
        assert_eq!(snapshot.ppid, 1);
        assert_eq!(snapshot.rss_kb, 1_006_400);
        assert_eq!(snapshot.elapsed, "05:04");
        assert!(snapshot.command.contains("--mode daemon"));
    }

    #[test]
    fn coordination_surface_lag_status_distinguishes_current_stale_and_missing() {
        assert_eq!(coordination_surface_lag_status(Some(7), 7), "current");
        assert_eq!(coordination_surface_lag_status(Some(6), 7), "stale");
        assert_eq!(coordination_surface_lag_status(None, 7), "missing");
    }
}
