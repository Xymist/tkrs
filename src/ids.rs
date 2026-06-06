use color_eyre::eyre::eyre;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use crate::Ticket;

const ID_LENGTH: usize = 5;

pub fn resolve_partial_id(tickets: &[Ticket], needle: &str) -> color_eyre::Result<String> {
    eprintln!("Resolving ID '{}' among {} tickets", needle, tickets.len());
    let mut matches = tickets
        .iter()
        .filter(|t| t.id().contains(needle))
        .map(|t| t.id().to_string())
        .collect::<Vec<_>>();
    matches.sort();
    matches.dedup();
    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => Err(eyre!("Error: ticket '{}' not found", needle)),
        _ => Err(eyre!(
            "Error: ambiguous ID '{}' matches multiple tickets",
            needle
        )),
    }
}

pub fn generate_id() -> color_eyre::Result<String> {
    let dir = std::env::current_dir()?;
    let dir_name = dir.file_name().unwrap_or_default().to_string_lossy();
    let segments: Vec<&str> = dir_name.split(&['-', '_'][..]).collect();

    let prefix: String = if segments.len() > 1 {
        segments.iter().filter_map(|s| s.chars().next()).collect()
    } else {
        dir_name.chars().take(3).collect()
    };

    let mut hasher = Sha256::new();
    let entropy = format!(
        "{}{}",
        std::process::id(),
        OffsetDateTime::now_utc().unix_timestamp()
    );
    hasher.update(entropy.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    let suffix: String = hash.chars().take(ID_LENGTH).collect();

    Ok(format!("{prefix}-{suffix}"))
}
