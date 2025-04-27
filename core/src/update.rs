// oss-porter-core/src/update.rs
use crate::utils::run_git_command;
use crate::{PorterError, ProjectConfig, Result};
use log::{debug, error, info, warn};
use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct CommitInfo {
  pub hash: String,
  pub subject: String,
}

/// Fetches latest changes for the internal repo and identifies relevant commits.
pub fn get_internal_commits_since(
  config: &ProjectConfig,
  since_ref: Option<&str>,
) -> Result<VecDeque<CommitInfo>> {
  // Return VecDeque for easy processing order
  let internal_repo = &config.internal_repo_path;
  let internal_branch = &config.internal_branch;
  let project_subdir = &config.project_subdir;

  info!(
    "Fetching updates for internal repository: {}",
    internal_repo.display()
  );
  // Optional: Add specific remote name if not 'origin'
  match run_git_command(&["fetch", "origin"], internal_repo) {
    Ok(_) => info!("Fetch successful."),
    Err(e) => warn!(
      "Failed to fetch internal repo (continuing with local state): {}",
      e
    ), // Non-fatal?
  }

  // Construct the commit range. Use origin/<branch> after fetch.
  // If since_ref is None, maybe list all commits on branch touching path? Risky.
  // Let's require a since_ref for now.
  let since_commit = since_ref.ok_or_else(|| {
    PorterError::GitOperation(
      "Cannot determine update range: no previous sync commit reference provided.".to_string(),
    )
  })?;

  let range = format!("{}..origin/{}", since_commit, internal_branch);
  info!(
    "Looking for commits in range '{}' affecting subdir '{}'",
    range,
    project_subdir.display()
  );

  // Use --no-merges to simplify history, --first-parent might also be useful sometimes
  // Format: hash<SEP>subject
  const HASH_SEP: &str = "<|OSS-PORTER-SEP|>";
  let _log_format = format!("%H{}%s", HASH_SEP);
  let log_args = &[
    "log",
    &range,
    "--no-merges",
    "--first-parent",           // Consider if this is desired - simplifies history
    "--pretty=format:%H%x00%s", // Use NULL separator for subject safety
    "--",                       // End of options, start of paths
    &project_subdir.to_string_lossy(), // Pathspec relative to repo root
  ];

  let log_output = run_git_command(log_args, internal_repo)?;
  let stdout = String::from_utf8_lossy(&log_output.stdout);

  let mut commits = VecDeque::new();
  // Process in reverse order so oldest is first
  for line in stdout.trim().lines().rev() {
    if line.is_empty() {
      continue;
    }
    let parts: Vec<&str> = line.splitn(2, '\x00').collect(); // Split by NULL
    if parts.len() == 2 {
      commits.push_back(CommitInfo {
        hash: parts[0].to_string(),
        subject: parts[1].to_string(),
      });
    } else {
      warn!("Could not parse commit log line: {}", line);
    }
  }

  info!("Found {} new candidate commits.", commits.len());
  Ok(commits)
}

/// Gets the formatted diff for a specific commit, relative to the project subdir.
pub fn get_commit_diff_relative(config: &ProjectConfig, commit_hash: &str) -> Result<String> {
  let internal_repo = &config.internal_repo_path;
  let project_subdir = &config.project_subdir;

  debug!("Getting relative diff for commit {}", commit_hash);
  // Show diff against parent (commit^!) relative to the subdir
  // Use color=always for potential terminal display later
  let diff_args = &[
    "diff",
    "--color=always", // Or remove if not needed downstream
    &format!("{}~..{}", commit_hash, commit_hash), // Diff against parent
    "--relative",     // Make paths relative to CWD
    &project_subdir.to_string_lossy(), // Path filter relative to CWD
  ];

  // Run the command from the internal repo root, paths in diff will be relative to project_subdir
  let diff_output = run_git_command(diff_args, internal_repo)?;
  let diff_str = String::from_utf8_lossy(&diff_output.stdout).to_string();
  Ok(diff_str)
  // Error handling: If commit_hash is invalid, run_git_command should return PorterError::GitCommand
}

#[derive(Debug, PartialEq, Eq)]
pub enum ApplyResult {
  Success,
  Conflict,
  Failure(String), // Contains stderr or error message
}

/// Attempts to apply a specific commit from the internal repo to the output repo using a patch.
pub fn apply_commit_to_output(config: &ProjectConfig, commit_hash: &str) -> Result<ApplyResult> {
  let internal_repo = &config.internal_repo_path;
  let project_subdir = &config.project_subdir;
  let output_path = &config.output_path;

  info!(
    "Generating patch for commit {} from internal repo relative to subdir '{}'",
    commit_hash,
    project_subdir.display()
  );

  // 1. Generate Patch relative to the subdirectory
  // Use `git format-patch` or `git diff` piped to a file. `format-patch` is generally better as it includes commit metadata.
  // We need the patch content relative to the *subdirectory* so it applies correctly in the output repo where the subdir *is* the root.
  let patch_args = &[
    "format-patch",
    "--stdout",                 // Option
    "-1",                       // How many commits
    commit_hash,                // The commit hash
    "--relative",               // Make paths relative to CWD (repo root)
    "--",                       // Separator
    &project_subdir.to_string_lossy(), // Pathspec filter
  ];

  // Run format-patch from the internal repo root
  let patch_output = run_git_command(patch_args, internal_repo)?;
  let patch_content = patch_output.stdout; // Patch content as bytes

  if patch_content.is_empty() {
    warn!("Generated empty patch for commit {}. This might mean changes were outside the subdirectory '{}' or only involved merges/empty changes. Skipping application.", commit_hash, project_subdir.display());
    // Treat as success because there's nothing to apply from the relevant subdir.
    return Ok(ApplyResult::Success);
  }

  // 2. Apply Patch using `git am` in the output repo
  // `git am` applies the patch and creates a commit using the metadata from the patch file.
  // It's generally preferred over `git apply` for syncing commits.
  // We need to feed the patch content via stdin.

  info!(
    "Applying patch for commit {} to output repo {}",
    commit_hash,
    output_path.display()
  );

  let mut apply_cmd = std::process::Command::new("git");
  apply_cmd.args(&[
    "am",
    "--keep-cr",
    "--committer-date-is-author-date",
    "--3way",
  ]); // Use 3-way merge for minor conflicts
  apply_cmd.current_dir(output_path);
  apply_cmd.stdin(std::process::Stdio::piped()); // Pipe stdin
  apply_cmd.stdout(std::process::Stdio::piped()); // Capture stdout/stderr
  apply_cmd.stderr(std::process::Stdio::piped());

  let mut child = apply_cmd.spawn().map_err(|e| PorterError::Io {
    source: e,
    path: output_path.to_path_buf(),
  })?;
  let mut child_stdin = child
    .stdin
    .take()
    .ok_or_else(|| PorterError::GitOperation("Failed to open stdin for git am".to_string()))?;

  // Write patch content to stdin in a separate thread to avoid deadlocks
  // (Though with small patches it might be fine without a thread)
  let write_handle = std::thread::spawn(move || {
    child_stdin
      .write_all(&patch_content)
      .map_err(|e| PorterError::Io {
        source: e,
        path: PathBuf::from("stdin"),
      }) // Use placeholder path
  });

  // Wait for the command to finish
  let apply_output = child.wait_with_output().map_err(|e| PorterError::Io {
    source: e,
    path: output_path.to_path_buf(),
  })?;

  // Check if writing to stdin failed
  match write_handle.join() {
    Ok(Ok(_)) => {} // Write succeeded
    Ok(Err(e)) => {
      error!("Failed to write patch to 'git am' stdin: {}", e);
      // Try to abort 'git am' if it might be stuck? Risky.
      // run_git_command(&["am", "--abort"], output_path).ok(); // Best effort abort
      return Err(e); // Return the write error
    }
    Err(_) => {
      // Panic from write thread
      error!("Patch writing thread panicked.");
      // run_git_command(&["am", "--abort"], output_path).ok(); // Best effort abort
      return Err(PorterError::GitOperation(
        "Patch writing thread panicked".to_string(),
      ));
    }
  }

  // Analyze the result of `git am`
  if apply_output.status.success() {
    info!(
      "Successfully applied patch for commit {} using 'git am'.",
      commit_hash
    );
    Ok(ApplyResult::Success)
  } else {
    let stdout = String::from_utf8_lossy(&apply_output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&apply_output.stderr).to_string();
    error!(
      "'git am' failed for commit {}. Status: {}",
      commit_hash, apply_output.status
    );
    error!("Stderr: {}", stderr);
    error!("Stdout: {}", stdout);

    // Check stderr/stdout for conflict indicators
    if stdout.contains("Patch failed to apply")
      || stderr.contains("Patch failed to apply")
      || stdout.contains("conflict")
      || stderr.contains("conflict")
      || stdout.contains("git am --continue")
      || stderr.contains("git am --continue")
    {
      warn!("'git am' resulted in conflicts for commit {}.", commit_hash);
      // Important: 'git am' leaves the repository in a conflicted state.
      // User MUST resolve and run `git am --continue` or `git am --abort`.
      Ok(ApplyResult::Conflict)
    } else {
      error!(
        "'git am' failed for commit {} with unexpected error.",
        commit_hash
      );
      // Abort the failed `am` attempt to clean up the repo state?
      warn!("Attempting to abort failed 'git am' session...");
      match run_git_command(&["am", "--abort"], output_path) {
        Ok(_) => info!("Successfully aborted failed 'git am' session."),
        Err(e) => warn!("Failed to abort 'git am' session: {}", e),
      }
      Ok(ApplyResult::Failure(stderr)) // General failure
    }
  }
}

/// Aborts an ongoing apply/am session in the output directory.
pub fn abort_apply_session(config: &ProjectConfig) -> Result<()> {
  // Try aborting both cherry-pick and am, as user might have used either manually
  warn!(
    "Aborting any ongoing apply/merge/rebase operation in {}",
    config.output_path.display()
  );
  // Use --quiet to suppress errors if no operation is in progress
  run_git_command(&["am", "--abort", "--quiet"], &config.output_path)?;
  run_git_command(&["cherry-pick", "--abort", "--quiet"], &config.output_path)?;
  run_git_command(&["rebase", "--abort", "--quiet"], &config.output_path)?; // Just in case
  run_git_command(&["merge", "--abort", "--quiet"], &config.output_path)?; // Just in case
  info!("Any potential apply/merge/rebase operation aborted.");
  Ok(())
}
