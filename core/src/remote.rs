use crate::extract::run_git_command; // Reuse the git command helper
use crate::{PorterError, ProjectConfig, Result};
use log::{error, info, warn};

/// Pushes the current state of the output repository to its configured public remote.
pub fn push_to_remote(project_id: &str, config: &ProjectConfig) -> Result<()> {
  info!("Attempting to push project '{}' to remote.", project_id);
  let output_path = &config.output_path;

  // Use configured public branch, defaulting to "main" via struct default in ProjectConfig
  let target_branch = &config.public_branch;
  info!("Configured public branch: {}", target_branch);

  // Check if output_path is a git repository
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
  info!("Target remote URL: {}", public_url);

  // 2. Check/Add 'origin' Remote
  let remote_output_res = run_git_command(&["remote", "-v"], output_path);
  let mut origin_exists_and_matches = false;

  match remote_output_res {
    Ok(remote_output) => {
      let remote_stdout = String::from_utf8_lossy(&remote_output.stdout);
      let mut origin_exists = false;
      let mut correct_url_found = false;

      for line in remote_stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts[0] == "origin" {
          origin_exists = true;
          if parts[1] == public_url {
            // Check if URL matches for either fetch or push
            correct_url_found = true;
          } else {
            // Origin exists but points elsewhere
            warn!(
              "Git remote 'origin' in '{}' points to '{}' instead of the configured '{}'.",
              output_path.display(),
              parts[1],
              public_url
            );
            // Allow proceeding if it points elsewhere? Or error out?
            // Let's error out for safety. User must fix manually.
            return Err(PorterError::GitOperation(format!(
                            "Git remote 'origin' in '{}' exists but points to the wrong URL ('{}'). Expected '{}'. Please fix manually.",
                            output_path.display(), parts[1], public_url
                        )));
          }
        }
      }
      if origin_exists && correct_url_found {
        origin_exists_and_matches = true;
        info!("Remote 'origin' already exists and points to the correct URL.");
      } else if !origin_exists {
        info!("Adding remote 'origin' with URL: {}", public_url);
        run_git_command(&["remote", "add", "origin", public_url], output_path)?;
        origin_exists_and_matches = true; // It now exists and matches
      }
      // If origin exists but URL was wrong, we already errored out.
    }
    Err(e) => {
      // Log the error but maybe try to add the remote anyway? Or just fail? Let's fail.
      error!("Failed to check git remotes: {}", e);
      return Err(e);
    }
  }

  if !origin_exists_and_matches {
    // This case should ideally be unreachable due to logic above, but added as safeguard
    return Err(PorterError::GitOperation(
      "Failed to verify or set up remote 'origin'.".to_string(),
    ));
  }

  // 3. Push the configured public branch
  // Assumes the local branch in output_path has the same name as target_branch
  info!(
    "Attempting to push local branch '{}' to remote 'origin/{}'",
    target_branch, target_branch
  );

  // Push the specific branch: <local_branch>:<remote_branch>
  // Use -u to set upstream tracking for the specified branch pair
  // Add --force option? No, dangerous. Let user handle non-fast-forwards.
  let push_output = run_git_command(
    &[
      "push",
      "-u",
      "origin",
      &format!("{}:{}", target_branch, target_branch),
    ],
    output_path,
  )?;

  // Check stderr for messages even on success (git sometimes prints to stderr)
  let push_stderr = String::from_utf8_lossy(&push_output.stderr);
  if !push_stderr.trim().is_empty() {
    // Often prints "Everything up-to-date" or branch tracking info here
    info!("Git push stderr:\n{}", push_stderr);
  }

  info!(
    "Successfully pushed branch '{}' for project '{}' to {}",
    target_branch, project_id, public_url
  );
  Ok(())
}
