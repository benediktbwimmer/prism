use anyhow::{anyhow, bail, Context, Result};
use prism_ir::PlanId;
use prism_query::Prism;
use serde::Serialize;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub(crate) fn handle_project_command(
    prism: &Prism,
    target: String,
    at: Option<String>,
    diff: Option<String>,
) -> Result<()> {
    let plan_id = parse_plan_target(&target)?;
    match (at, diff) {
        (Some(at), None) => {
            let as_of = parse_projection_timestamp(&at)?;
            let projection = prism.plan_projection_at(&plan_id, as_of).ok_or_else(|| {
                anyhow!(
                    "no historical plan projection found for `{}` at `{at}`",
                    plan_id.0
                )
            })?;
            print_json(&projection)
        }
        (None, Some(diff)) => {
            let (from, to) = parse_projection_diff_window(&diff)?;
            let projection = prism.plan_projection_diff(&plan_id, from, to);
            print_json(&projection)
        }
        (None, None) => bail!("project requires either `--at <timestamp>` or `--diff <from..to>`"),
        (Some(_), Some(_)) => bail!("project accepts either `--at` or `--diff`, not both"),
    }
}

fn parse_plan_target(value: &str) -> Result<PlanId> {
    if !value.starts_with("plan:") {
        bail!("project target must be a plan id like `plan:01kn...`");
    }
    Ok(PlanId::new(value.to_string()))
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn parse_projection_diff_window(value: &str) -> Result<(u64, u64)> {
    let (from, to) = value
        .split_once("..")
        .ok_or_else(|| anyhow!("diff window must look like `<from>..<to>`"))?;
    Ok((
        parse_projection_timestamp(from)?,
        parse_projection_timestamp(to)?,
    ))
}

fn parse_projection_timestamp(value: &str) -> Result<u64> {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("now") {
        return unix_timestamp(OffsetDateTime::now_utc());
    }
    if let Some(relative) = trimmed.strip_prefix("now-") {
        let now = OffsetDateTime::now_utc();
        let seconds = parse_relative_seconds(relative)?;
        return unix_timestamp(now - time::Duration::seconds(seconds));
    }
    if let Ok(parsed) = trimmed.parse::<u64>() {
        return Ok(parsed);
    }
    let parsed = OffsetDateTime::parse(trimmed, &Rfc3339).with_context(|| {
        format!("failed to parse timestamp `{trimmed}` as unix seconds, `now-...`, or RFC3339")
    })?;
    unix_timestamp(parsed)
}

fn unix_timestamp(value: OffsetDateTime) -> Result<u64> {
    u64::try_from(value.unix_timestamp())
        .map_err(|_| anyhow!("timestamp `{}` is before the unix epoch", value))
}

fn parse_relative_seconds(value: &str) -> Result<i64> {
    if value.len() < 2 {
        bail!(
            "relative timestamp `{value}` must include a numeric value and unit, for example `now-4h`"
        );
    }
    let (amount, unit) = value.split_at(value.len() - 1);
    let amount = amount
        .parse::<i64>()
        .with_context(|| format!("failed to parse relative duration amount in `{value}`"))?;
    let multiplier = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 60 * 60,
        "d" => 24 * 60 * 60,
        _ => bail!("unsupported relative duration unit `{unit}` in `{value}`; use s, m, h, or d"),
    };
    Ok(amount * multiplier)
}

#[cfg(test)]
mod tests {
    use super::{parse_projection_diff_window, parse_projection_timestamp};

    #[test]
    fn projection_timestamp_accepts_unix_seconds() {
        assert_eq!(parse_projection_timestamp("123").unwrap(), 123);
    }

    #[test]
    fn projection_timestamp_accepts_rfc3339() {
        assert_eq!(
            parse_projection_timestamp("2026-04-02T12:00:00Z").unwrap(),
            1_775_131_200
        );
    }

    #[test]
    fn projection_diff_window_requires_separator() {
        assert!(parse_projection_diff_window("now-4h").is_err());
    }
}
