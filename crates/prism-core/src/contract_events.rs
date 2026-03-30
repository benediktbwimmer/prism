use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use anyhow::{Context, Result};
use prism_projections::{curated_contracts_from_events, ContractEvent, ContractPacket};

use crate::util::repo_contract_events_path;

pub(crate) fn append_repo_contract_event(root: &Path, event: &ContractEvent) -> Result<()> {
    let path = repo_contract_events_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    serde_json::to_writer(&mut file, event)?;
    file.write_all(b"\n")?;
    Ok(())
}

pub(crate) fn load_repo_contract_events(root: &Path) -> Result<Vec<ContractEvent>> {
    let path = repo_contract_events_path(root);
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = File::open(&path)?;
    let reader = BufReader::new(file);
    let mut events = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let event = serde_json::from_str::<ContractEvent>(&line).with_context(|| {
            format!(
                "failed to parse contract event on line {} in {}",
                index + 1,
                path.display()
            )
        })?;
        events.push(event);
    }
    Ok(events)
}

pub(crate) fn load_repo_curated_contracts(root: &Path) -> Result<Vec<ContractPacket>> {
    Ok(curated_contracts_from_events(&load_repo_contract_events(
        root,
    )?))
}
