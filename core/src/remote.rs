use crate::extract::run_git_command; // Reuse the git command helper
use crate::{PorterError, ProjectConfig, Result};
use log::{debug, info, warn};
use std::path::Path;

/// Pushes the current state of the output repository to its configured public remote.
pub fn push_to_remote(project_id: &str, config: &ProjectConfig) -> Result<()> {
  info!("Attempting to push project '{}' to remote.", project_id);
  let output_path = &config.output_path;

  if !output_path.join(".git").exists() {
    return Err(PorterError::GitOperation(format!(
      "Output path '{}' is not a Git repository. Cannot push.",
      output_path.display()
    )));
  }

  // 1. Check for Public Repo URL
  let public_url = config.public_repo_url.as_ref().ok_or_else(|| {
    PorterError::Config(format!(
      "Project '{}' does not have 'public_repo_url' configured. Cannot push.",
      project_id
    ))
  })?;

  // 2. Check/Add 'origin' Remote
  let remote_output = run_git_command(&["remote", "-v"], output_path)?;
  let remote_stdout = String::from_utf8_lossy(&remote_output.stdout);
  let mut origin_exists = false;
  let mut origin_matches = false;

  for line in remote_stdout.lines() {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() >= 2 && parts[0] == "origin" {
      origin_exists = true;
      if parts[1] == public_url {
        origin_matches = true;
        // Check both fetch and push URLs if needed, this checks the first one found
        break;
      } else {
        // Origin exists but points elsewhere
        return Err(PorterError::GitOperation(format!(
                     "Git remote 'origin' in '{}' already exists but points to '{}' instead of the configured '{}'. Please fix manually.",
                     output_path.display(), parts[1], public_url
                 )));
        // Add --force option later to allow changing it? Risky.
      }
    }
  }

  if !origin_exists {
    info!("Adding remote 'origin' with URL: {}", public_url);
    run_git_command(&["remote", "add", "origin", public_url], output_path)?;
  } else if origin_matches {
    info!("Remote 'origin' already exists and points to the correct URL.");
  }
  // The case where origin exists but doesn't match is handled by the error above

  // 3. Determine Current Branch (simple approach: assume 'main' or 'master')
  // A more robust approach involves parsing `git branch --show-current` or `git symbolic-ref HEAD`
  let current_branch = "main"; // Hardcoded for now, consider improving
  info!("Attempting to push branch '{}' to origin.", current_branch);
  // TODO: Find current branch dynamically instead of hardcoding 'main'

  // 4. Push to Remote
  // Use -u to set upstream tracking on the first push
  let push_output = run_git_command(&["push", "-u", "origin", current_branch], output_path)?;

  // Check stderr for messages even on success (git sometimes prints to stderr)
  let push_stderr = String::from_utf8_lossy(&push_output.stderr);
  if !push_stderr.is_empty() {
    info!("Git push stderr:\n{}", push_stderr); // Log informational messages
  }

  info!(
    "Successfully pushed project '{}' to {}",
    project_id, public_url
  );
  Ok(())
}
