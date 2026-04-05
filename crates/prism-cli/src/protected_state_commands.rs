use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use prism_core::{
    diagnose_protected_state, export_protected_state_trust_material,
    import_protected_state_trust_material, inspect_legacy_path_identity_state,
    inspect_repo_published_plan_artifacts, migrate_legacy_protected_repo_state,
    quarantine_protected_state_stream, reconcile_protected_state_stream,
    regenerate_repo_snapshot_derived_artifacts, repair_legacy_path_identity_state,
    repair_protected_state_stream_to_last_valid, repair_repo_published_plan_artifacts,
    verify_protected_state, LegacyPathIdentityRepairReport, ProtectedStateQuarantineReport,
    ProtectedStateReconcileReport, ProtectedStateRepairReport, ProtectedStateStreamReport,
    ProtectedStateTrustExport, ProtectedStateTrustImportReport, ProtectedStateVerifyReport,
    PublishedPlanArtifactRepairReport,
};

use crate::cli::{ProtectedStateCommand, ProtectedStateTrustCommand};
use crate::git_support::{
    install_repo_git_support, run_derived_merge_driver, run_snapshot_derived_merge_driver,
    run_stream_merge_driver,
};

pub(crate) fn handle_protected_state_command(
    root: &Path,
    command: ProtectedStateCommand,
) -> Result<()> {
    match command {
        ProtectedStateCommand::InstallGitSupport => {
            install_repo_git_support(root)?;
        }
        ProtectedStateCommand::Verify { stream } => {
            let report = match stream.as_deref() {
                Some(stream) => verify_single_stream(root, stream)?,
                None => verify_protected_state(root)?,
            };
            print_verify_report(&report);
            if !report.all_verified {
                bail!(
                    "{} protected stream(s) were not verified",
                    report.non_verified_stream_count
                );
            }
        }
        ProtectedStateCommand::Diagnose { stream } => {
            let reports = diagnose_protected_state(root, stream.as_deref())?;
            for (index, report) in reports.iter().enumerate() {
                if index > 0 {
                    println!();
                }
                print_diagnostic_report(report);
            }
        }
        ProtectedStateCommand::MigrateSign => {
            let report = migrate_legacy_protected_repo_state(root)?;
            if report.migrated_stream_count == 0 {
                println!("no legacy protected streams required migration");
            } else {
                println!(
                    "migrated {} protected stream(s), {} event(s)",
                    report.migrated_stream_count, report.migrated_event_count
                );
                for path in report.migrated_paths {
                    println!("{}", path.display());
                }
            }
        }
        ProtectedStateCommand::Trust { command } => {
            handle_protected_state_trust_command(root, command)?;
        }
        ProtectedStateCommand::Quarantine { stream } => {
            let report = quarantine_protected_state_stream(root, &stream)?;
            print_quarantine_report(&report);
        }
        ProtectedStateCommand::Repair {
            stream,
            to_last_valid,
        } => {
            if !to_last_valid {
                bail!("protected-state repair currently requires `--to-last-valid`");
            }
            let report = repair_protected_state_stream_to_last_valid(root, &stream)?;
            print_repair_report(&report);
        }
        ProtectedStateCommand::RepairPublishedPlans { check } => {
            let report = if check {
                inspect_repo_published_plan_artifacts(root)?
            } else {
                repair_repo_published_plan_artifacts(root)?
            };
            print_published_plan_repair_report(&report);
            if check && report.redundant_edge_add_count != 0 {
                bail!(
                    "{} redundant published plan edge_added event(s) need repair",
                    report.redundant_edge_add_count
                );
            }
        }
        ProtectedStateCommand::RepairSnapshotArtifacts => {
            regenerate_repo_snapshot_derived_artifacts(root)?;
            println!("regenerated snapshot-derived PRISM artifacts");
        }
        ProtectedStateCommand::RepairPathIdentity { check } => {
            let report = if check {
                inspect_legacy_path_identity_state(root)?
            } else {
                repair_legacy_path_identity_state(root)?
            };
            print_path_identity_repair_report(&report, check);
            if check && report.total_entries_needing_repair != 0 {
                bail!(
                    "{} path-identity record(s) need repair",
                    report.total_entries_needing_repair
                );
            }
        }
        ProtectedStateCommand::ReconcileStream {
            stream,
            accepted_head,
        } => {
            let report = reconcile_protected_state_stream(root, &stream, &accepted_head)?;
            print_reconcile_report(&report);
        }
        ProtectedStateCommand::MergeDriverStream {
            ancestor,
            current,
            other,
            path,
        } => {
            run_stream_merge_driver(root, &ancestor, &current, &other, &path)?;
        }
        ProtectedStateCommand::MergeDriverDerived {
            ancestor,
            current,
            other,
            path,
        } => {
            run_derived_merge_driver(root, &ancestor, &current, &other, &path)?;
        }
        ProtectedStateCommand::MergeDriverSnapshotDerived {
            ancestor,
            current,
            other,
            path,
        } => {
            run_snapshot_derived_merge_driver(root, &ancestor, &current, &other, &path)?;
        }
    }

    Ok(())
}

fn verify_single_stream(root: &Path, stream: &str) -> Result<ProtectedStateVerifyReport> {
    let streams = diagnose_protected_state(root, Some(stream))?;
    let non_verified_stream_count = streams
        .iter()
        .filter(|stream| stream.verification_status != "Verified")
        .count();
    Ok(ProtectedStateVerifyReport {
        all_verified: non_verified_stream_count == 0,
        non_verified_stream_count,
        streams,
    })
}

fn handle_protected_state_trust_command(
    root: &Path,
    command: ProtectedStateTrustCommand,
) -> Result<()> {
    match command {
        ProtectedStateTrustCommand::Export {
            bundle_id,
            output,
            root_output,
        } => {
            let export = export_protected_state_trust_material(root, bundle_id.as_deref())?;
            write_trust_export(&export, &output, root_output.as_ref())?;
        }
        ProtectedStateTrustCommand::Import {
            bundle,
            root: root_file,
        } => {
            let bundle_json = fs::read_to_string(&bundle)
                .with_context(|| format!("failed to read {}", bundle.display()))?;
            let root_json = match root_file {
                Some(path) => Some(
                    fs::read_to_string(&path)
                        .with_context(|| format!("failed to read {}", path.display()))?,
                ),
                None => None,
            };
            let report =
                import_protected_state_trust_material(root, &bundle_json, root_json.as_deref())?;
            print_trust_import_report(&report);
        }
    }

    Ok(())
}

fn write_trust_export(
    export: &ProtectedStateTrustExport,
    output: &Path,
    root_output: Option<&PathBuf>,
) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(output, &export.bundle_json)
        .with_context(|| format!("failed to write {}", output.display()))?;
    let root_output = root_output
        .cloned()
        .unwrap_or_else(|| derived_root_output_path(output));
    if let Some(parent) = root_output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&root_output, &export.root_json)
        .with_context(|| format!("failed to write {}", root_output.display()))?;
    println!(
        "exported trust bundle {} to {}",
        export.bundle_id,
        output.display()
    );
    println!(
        "exported trusted root {} to {}",
        export.authority_root_id,
        root_output.display()
    );
    Ok(())
}

fn derived_root_output_path(output: &Path) -> PathBuf {
    let stem = output
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("trust-bundle");
    output.with_file_name(format!("{stem}.root.json"))
}

fn print_verify_report(report: &ProtectedStateVerifyReport) {
    for stream in &report.streams {
        println!(
            "{}\t{}\t{}",
            stream.verification_status, stream.stream_id, stream.protected_path
        );
        if let Some(summary) = stream.diagnostic_summary.as_deref() {
            println!("  {}", summary);
        }
    }
    println!(
        "verified {} protected stream(s); {} non-verified",
        report.streams.len(),
        report.non_verified_stream_count
    );
}

fn print_diagnostic_report(report: &ProtectedStateStreamReport) {
    println!("stream: {}", report.stream_id);
    println!("path: {}", report.protected_path);
    println!("status: {}", report.verification_status);
    println!(
        "last verified event: {}",
        report.last_verified_event_id.as_deref().unwrap_or("<none>")
    );
    println!(
        "last verified entry hash: {}",
        report
            .last_verified_entry_hash
            .as_deref()
            .unwrap_or("<none>")
    );
    println!(
        "trust bundle: {}",
        report.trust_bundle_id.as_deref().unwrap_or("<none>")
    );
    if let Some(code) = report.diagnostic_code.as_deref() {
        println!("diagnostic code: {}", code);
    }
    if let Some(summary) = report.diagnostic_summary.as_deref() {
        println!("diagnostic summary: {}", summary);
    }
    if let Some(hint) = report.repair_hint.as_deref() {
        println!("repair hint: {}", hint);
    }
}

fn print_trust_import_report(report: &ProtectedStateTrustImportReport) {
    println!(
        "imported trust bundle {} for authority root {}",
        report.bundle_id, report.authority_root_id
    );
    if report.pinned_root_supplied {
        println!("used explicit trust root pinning during import");
    }
}

fn print_quarantine_report(report: &ProtectedStateQuarantineReport) {
    println!("quarantined {}", report.stream_id);
    println!("source: {}", report.protected_path);
    println!("archive: {}", report.quarantined_path);
}

fn print_repair_report(report: &ProtectedStateRepairReport) {
    println!("repaired {}", report.stream_id);
    println!("source: {}", report.protected_path);
    println!("quarantine: {}", report.quarantined_path);
    println!(
        "restored head: {}",
        report.restored_event_id.as_deref().unwrap_or("<empty>")
    );
    println!("restored records: {}", report.restored_record_count);
}

fn print_reconcile_report(report: &ProtectedStateReconcileReport) {
    println!("reconciled {}", report.stream_id);
    println!("source: {}", report.protected_path);
    println!("quarantine: {}", report.quarantined_path);
    println!("accepted head: {}", report.accepted_head_event_id);
    println!("restored records: {}", report.restored_record_count);
}

fn print_published_plan_repair_report(report: &PublishedPlanArtifactRepairReport) {
    for entry in &report.entries {
        println!(
            "{}\t{}\tremoved_redundant_edge_adds={}\tevents={}=>{}\tlegacy_skipped={}",
            if entry.repaired {
                "repaired"
            } else if entry.redundant_edge_add_count > 0 {
                "needs_repair"
            } else {
                "ok"
            },
            entry.protected_path,
            entry.redundant_edge_add_count,
            entry.event_count_before,
            entry.event_count_after,
            entry.skipped_legacy_stream,
        );
    }
    println!(
        "scanned {} published plan stream(s); repaired {}; removed {} redundant edge_added event(s)",
        report.scanned_plan_count, report.repaired_plan_count, report.redundant_edge_add_count
    );
}

fn print_path_identity_repair_report(report: &LegacyPathIdentityRepairReport, check: bool) {
    for target in &report.targets {
        let status = if target.entries_needing_repair == 0 {
            "clean"
        } else if check {
            "needs_repair"
        } else if target.repaired {
            "repaired"
        } else {
            "pending"
        };
        println!(
            "{}\t{}\t{}\t{} scanned, {} need repair",
            status,
            target.label,
            target.location,
            target.scanned_entry_count,
            target.entries_needing_repair
        );
    }
    println!(
        "scanned {} target(s), {} entry(s); {} need repair, {} repaired",
        report.scanned_target_count,
        report.total_scanned_entry_count,
        report.total_entries_needing_repair,
        report.repaired_target_count
    );
}
