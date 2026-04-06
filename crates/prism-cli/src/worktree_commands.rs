use std::io::{self, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Result};
use clap::ValueEnum;
use prism_core::{hydrate_workspace_session, list_registered_worktrees, PrismPaths, WorktreeMode};
use prism_ir::SessionId;

use crate::cli::WorktreeCommand;
use crate::operator_auth::require_active_human_session;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum WorktreeModeArg {
    Human,
    Agent,
}

pub(crate) fn handle_worktree_command(root: &Path, command: WorktreeCommand) -> Result<()> {
    match command {
        WorktreeCommand::List => handle_worktree_list(root),
        WorktreeCommand::Register { label, mode } => handle_worktree_register(root, label, mode),
        WorktreeCommand::Relabel { label } => handle_worktree_relabel(root, label),
        WorktreeCommand::Takeover { reason } => handle_worktree_takeover(root, reason),
    }
}

fn handle_worktree_list(root: &Path) -> Result<()> {
    let paths = PrismPaths::for_workspace_root(root)?;
    let current_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let registrations = list_registered_worktrees(paths.home_root())?;
    if registrations.is_empty() {
        println!("no registered worktrees");
        return Ok(());
    }

    for registration in registrations {
        let marker = if registration.canonical_root == current_root {
            "*"
        } else {
            "-"
        };
        println!(
            "{marker} {} [{}] {}",
            registration.agent_label,
            render_worktree_mode(registration.mode),
            registration.registered_worktree_id,
        );
        println!("  root = {}", registration.canonical_root.display());
        if let Some(branch_ref) = registration.branch_ref.as_deref() {
            println!("  branch = {branch_ref}");
        }
    }
    Ok(())
}

fn handle_worktree_register(
    root: &Path,
    label: Option<String>,
    mode: Option<WorktreeModeArg>,
) -> Result<()> {
    let mut auth = require_active_human_session(root)?;
    let label = resolve_worktree_label(label, "Worktree label")?;
    let mode = resolve_worktree_mode(mode)?;
    let paths = PrismPaths::for_workspace_root(root)?;
    let registration = paths.register_worktree(&label, mode)?;
    auth.persist()?;
    print_registration("registered worktree", &registration);
    Ok(())
}

fn handle_worktree_relabel(root: &Path, label: Option<String>) -> Result<()> {
    let mut auth = require_active_human_session(root)?;
    let paths = PrismPaths::for_workspace_root(root)?;
    let existing = paths.worktree_registration()?.ok_or_else(|| {
        anyhow!("current worktree is not registered; run `prism worktree register` first")
    })?;
    let label = resolve_worktree_label(label, "New worktree label")?;
    let registration = paths.register_worktree(&label, existing.mode)?;
    auth.persist()?;
    print_registration("relabeled worktree", &registration);
    Ok(())
}

fn handle_worktree_takeover(root: &Path, reason: Option<String>) -> Result<()> {
    let mut auth = require_active_human_session(root)?;
    let paths = PrismPaths::for_workspace_root(root)?;
    if paths.worktree_registration()?.is_none() {
        bail!("current worktree is not registered; run `prism worktree register` first");
    }
    let reason = resolve_worktree_label(reason, "Takeover reason")?;
    let session = hydrate_workspace_session(root)?;
    let slot = session.take_over_worktree_mutator_slot(
        &auth.authenticated,
        &SessionId::new(format!(
            "session:worktree-takeover:{}",
            current_unix_timestamp()
        )),
        Some(reason.as_str()),
    )?;
    auth.persist()?;
    println!("took over mutator slot");
    println!("worktree_id = {}", slot.worktree_id);
    println!("session_id = {}", slot.session_id);
    println!("principal_id = {}", slot.principal_id);
    if let Some(reason) = slot.takeover_reason.as_deref() {
        println!("takeover_reason = {}", reason);
    }
    Ok(())
}

fn resolve_worktree_label(label: Option<String>, prompt: &str) -> Result<String> {
    match label {
        Some(label) if !label.trim().is_empty() => Ok(label.trim().to_string()),
        Some(_) => bail!("worktree label must not be empty"),
        None => prompt_nonempty(prompt),
    }
}

fn resolve_worktree_mode(mode: Option<WorktreeModeArg>) -> Result<WorktreeMode> {
    match mode {
        Some(mode) => Ok(map_worktree_mode_arg(mode)),
        None => loop {
            let value = prompt_nonempty("Worktree mode [human/agent]")?;
            match value.to_ascii_lowercase().as_str() {
                "human" => return Ok(WorktreeMode::Human),
                "agent" => return Ok(WorktreeMode::Agent),
                _ => {
                    eprintln!("invalid worktree mode `{value}`; enter `human` or `agent`");
                }
            }
        },
    }
}

fn prompt_nonempty(prompt: &str) -> Result<String> {
    let mut stdout = io::stdout();
    write!(stdout, "{prompt}: ")?;
    stdout.flush()?;
    let mut buffer = String::new();
    io::stdin().read_line(&mut buffer)?;
    let value = buffer.trim();
    if value.is_empty() {
        bail!("{prompt} must not be empty");
    }
    Ok(value.to_string())
}

fn print_registration(prefix: &str, registration: &prism_core::WorktreeRegistrationRecord) {
    println!("{prefix}");
    println!("worktree_id = {}", registration.worktree_id);
    println!("agent_label = {}", registration.agent_label);
    println!("mode = {}", render_worktree_mode(registration.mode));
}

fn render_worktree_mode(mode: WorktreeMode) -> &'static str {
    match mode {
        WorktreeMode::Human => "human",
        WorktreeMode::Agent => "agent",
    }
}

fn map_worktree_mode_arg(mode: WorktreeModeArg) -> WorktreeMode {
    match mode {
        WorktreeModeArg::Human => WorktreeMode::Human,
        WorktreeModeArg::Agent => WorktreeMode::Agent,
    }
}

fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("current time should be after unix epoch")
        .as_secs()
}
