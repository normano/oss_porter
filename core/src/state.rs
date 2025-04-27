// oss-porter-core/src/state.rs
use crate::utils::run_git_command; // Use from utils
use crate::{PorterError, ProjectConfig, Result};
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::{
  fs,
  io::Write,
  path::PathBuf,
};

#[derive(Serialize, Deserialize, Debug, Default)]
struct StateFileContent {
  last_synced_internal_commit: Option<String>,
}

pub const STATE_FILE_NAME: &str = ".oss_porter_state.toml";

// Helper to get the full path to the state file within the internal project subdir
pub fn get_internal_state_file_path(config: &ProjectConfig) -> PathBuf {
  config
    .internal_repo_path
    .join(&config.project_subdir)
    .join(STATE_FILE_NAME)
}

/// Reads the last synced commit hash from the state file in the internal repo.
/// Returns Ok(None) if the file doesn't exist or the commit hash is not set.
pub fn read_last_synced_commit(config: &ProjectConfig) -> Result<Option<String>> {
  let state_file_path = get_internal_state_file_path(config);
  debug!("Reading sync state from: {}", state_file_path.display());

  if !state_file_path.exists() {
    info!(
      "Sync state file not found at {}. Assuming no prior sync.",
      state_file_path.display()
    );
    return Ok(None); // No state file means no previous sync state recorded
  }

  let content = fs::read_to_string(&state_file_path).map_err(|e| PorterError::Io {
    source: e,
    path: state_file_path.clone(),
  })?;

  if content.trim().is_empty() {
    warn!(
      "Sync state file {} is empty. Assuming no prior sync.",
      state_file_path.display()
    );
    return Ok(None); // Empty file
  }

  let state: StateFileContent = toml::from_str(&content).map_err(|e| PorterError::TomlParse {
    source: e,
    path: state_file_path,
  })?;

  // Normalize empty string to None
  match state.last_synced_internal_commit {
    Some(s) if s.trim().is_empty() => {
      warn!("Sync state file contains empty commit hash. Assuming no prior sync.");
      Ok(None)
    }
    other => Ok(other),
  }
}

/// Writes the last synced commit hash to the state file in the internal repo.
/// Overwrites existing file. Does NOT commit the change.
pub fn write_last_synced_commit(config: &ProjectConfig, commit_hash: Option<&str>) -> Result<()> {
  let state_file_path = get_internal_state_file_path(config);
  let hash_to_write = commit_hash.map(|s| s.to_string()); // Convert Option<&str> to Option<String>
  debug!(
    "Writing sync state {:?} to: {}",
    &hash_to_write,
    state_file_path.display()
  );

  let state = StateFileContent {
    last_synced_internal_commit: hash_to_write,
  };

  let toml_string = toml::to_string_pretty(&state).map_err(|e| PorterError::TomlSerialize(e))?;

  // Ensure parent directory exists (should normally be the project subdir)
  if let Some(parent) = state_file_path.parent() {
    fs::create_dir_all(parent).map_err(|e| PorterError::Io {
      source: e,
      path: parent.to_path_buf(),
    })?;
  }

  let mut file = fs::File::create(&state_file_path).map_err(|e| PorterError::Io {
    source: e,
    path: state_file_path.clone(),
  })?;
  file
    .write_all(toml_string.as_bytes())
    .map_err(|e| PorterError::Io {
      source: e,
      path: state_file_path,
    })?;

  Ok(())
}

/// Commits the state file change in the internal repository.
pub fn commit_state_file_change(config: &ProjectConfig, commit_hash: Option<&str>) -> Result<()> {
  // Use PathBuf::from for consistent path separator handling
  let state_file_rel_path = PathBuf::from(STATE_FILE_NAME);
  let internal_project_dir = config.internal_repo_path.join(&config.project_subdir);

  let commit_hash_msg = commit_hash.unwrap_or("<none>"); // Message if hash is cleared
  let commit_message = format!(
    "chore(oss-porter): Update sync state to {}",
    commit_hash_msg
  );

  info!(
    "Committing state file change in internal repo: {}",
    internal_project_dir.display()
  );

  // Check if state file is actually modified? Optional but good practice.
  let status_output = run_git_command(
    &[
      "status",
      "--porcelain",
      &state_file_rel_path.to_string_lossy(),
    ],
    &internal_project_dir,
  )?;
  if String::from_utf8_lossy(&status_output.stdout)
    .trim()
    .is_empty()
  {
    info!(
      "State file {} not modified, skipping commit.",
      STATE_FILE_NAME
    );
    return Ok(());
  }

  // Stage the specific state file relative to the internal project dir
  run_git_command(
    &["add", &state_file_rel_path.to_string_lossy()],
    &internal_project_dir,
  )?;

  // Commit
  run_git_command(&["commit", "-m", &commit_message], &internal_project_dir)?;

  info!("Successfully committed state file update in internal repository.");
  Ok(())
}
