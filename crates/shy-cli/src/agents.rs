//! Where members' data lives. The default member uses one folder; named
//! agents each get their own, so several agents can be separate members.

use std::path::PathBuf;

use anyhow::{Context, Result, ensure};
use shy_core::config::WorkspaceConfig;

const MAX_NAME_LEN: usize = 40;

fn data_home() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .context("cannot find a data directory; pass --dir")?;
    Ok(base.join("shyspace"))
}

pub fn default_member_dir() -> Result<PathBuf> {
    Ok(data_home()?.join("cli"))
}

fn agents_dir() -> Result<PathBuf> {
    Ok(data_home()?.join("agents"))
}

/// The folder of the agent called `name` (letters, digits and dashes).
pub fn agent_dir(name: &str) -> Result<PathBuf> {
    ensure!(
        !name.is_empty()
            && name.len() <= MAX_NAME_LEN
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
        "agent names use lowercase letters, digits and dashes (up to {MAX_NAME_LEN})"
    );
    Ok(agents_dir()?.join(name))
}

/// Every named agent, with the workspace it belongs to (if it joined one).
pub fn list() -> Result<Vec<(String, Option<String>)>> {
    let dir = agents_dir()?;
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut agents = Vec::new();
    for entry in std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let name = entry.file_name().to_string_lossy().into_owned();
            agents.push((name, WorkspaceConfig::peek_name(&entry.path())?));
        }
    }
    agents.sort();
    Ok(agents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_names_are_simple() {
        assert!(
            agent_dir("reviewer")
                .unwrap()
                .ends_with("shyspace/agents/reviewer")
        );
        assert!(agent_dir("code-bot-2").is_ok());
        for bad in ["", "Reviewer", "../escape", "a b", &"x".repeat(41)] {
            assert!(agent_dir(bad).is_err(), "{bad:?}");
        }
    }
}
