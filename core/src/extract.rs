use crate::{ExtractionResult, HistoryMode, PorterError, ProjectConfig, Result};
use fs_extra::dir::{
  copy as copy_dir, move_dir, CopyOptions, TransitProcess, TransitProcessResult,
}; // Added move_dir, Transit*
use log::{debug, error, info, warn};
use regex::Regex;
use std::{
  ffi::OsStr, // For check_tool_exists
  fs,
  path::{Path, PathBuf},
  process::{Command, Output, Stdio}, // Added Stdio
};
use tempfile::TempDir;
use walkdir::WalkDir;

// --- Helper Functions ---

/// Checks if a command-line tool exists in the system's PATH.
fn check_tool_exists(tool_name: &str) -> Result<()> {
  Command::new(tool_name)
    .arg("--version") // Most tools support --version or similar
    .stdout(Stdio::null()) // Don't capture output unless needed for debugging
    .stderr(Stdio::null())
    .status()
    .map_err(|e| {
      if e.kind() == std::io::ErrorKind::NotFound {
        PorterError::ToolNotFound(tool_name.to_string())
      } else {
        PorterError::Io(e)
      }
    })?; // Check if the command even starts
  Ok(())
}

/// Runs a command in the specified directory, capturing output.
/// Use this for commands where you need the output string or detailed errors.
fn run_command_capture(cmd_name: &str, args: &[&str], cwd: &Path) -> Result<Output> {
  let cmd_str = format!("{} {}", cmd_name, args.join(" "));
  debug!(
    "Running command: '{}' in directory: {}",
    cmd_str,
    cwd.display()
  );

  let output = Command::new(cmd_name)
    .args(args)
    .current_dir(cwd)
    .output()?; // Propagates std::io::Error as PorterError::Io

  if !output.status.success() {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    error!("Command failed: {}", cmd_str);
    error!("Stderr: {}", stderr);
    error!("Stdout: {}", stdout);
    // Provide a slightly more specific error type if desired
    return Err(PorterError::GitCommand {
      cmd: cmd_str,
      stdout,
      stderr,
    }); // Reusing GitCommand for general command failures
  }
  debug!("Command successful: {}", cmd_str);
  Ok(output)
}

/// Runs a git command specifically.
pub(crate) fn run_git_command(args: &[&str], cwd: &Path) -> Result<Output> {
  run_command_capture("git", args, cwd)
}

/// Very basic secret scanning using simple regexes.
/// Returns a list of findings (file path + potential secret type).
pub(crate) fn scan_secrets_basic(dir: &Path) -> Result<Vec<String>> {
  info!("Starting basic secret scan in {}", dir.display());
  let mut findings = Vec::new();
  // Example patterns (very naive - enhance later or use external tools)
  let patterns = [
    Regex::new(r#"(?i)(api_?key|secret|password)\s*[:=]\s*['"]\S+['"]"#).unwrap(),
    Regex::new(r#"([A-Za-z0-9+/]{40,})"#).unwrap(), // long base64-like strings
  ];

  for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
    let path = entry.path();
    if path.is_file() {
      // Skip target directory and potentially other binary files/vendored code
      if path
        .components()
        .any(|comp| comp.as_os_str() == "target" || comp.as_os_str() == ".git")
      {
        continue;
      }
      if let Ok(content) = fs::read_to_string(path) {
        for (i, line) in content.lines().enumerate() {
          for pattern in &patterns {
            if pattern.is_match(line) {
              let finding = format!("Potential secret found in {}:{}", path.display(), i + 1);
              warn!("{}", finding);
              findings.push(finding);
              break; // Only report first match per line
            }
          }
        }
      }
    }
  }
  info!(
    "Basic secret scan completed. Found {} potential issues.",
    findings.len()
  );
  Ok(findings)
}

/// Adds a license file if specified and doesn't exist.
fn add_license_file(license_id: Option<&str>, output_path: &Path) -> Result<()> {
  if let Some(id) = license_id {
    // Very basic check - assumes license name matches file name (e.g., "MIT" -> "LICENSE-MIT")
    // A better approach would use SPDX IDs and fetch/generate license text.
    let license_file_name = format!("LICENSE-{}", id.to_uppercase());
    let license_path = output_path.join(&license_file_name);
    let generic_license_path = output_path.join("LICENSE");

    if !license_path.exists() && !generic_license_path.exists() {
      info!("Adding license file for {} (placeholder)", id);
      // Placeholder content - replace with actual license text fetching later
      let content = format!("Placeholder for {} License Text.", id);
      fs::write(&license_path, content)?;
      info!("Created license file: {}", license_path.display());
    } else {
      info!("License file already exists, skipping creation.");
    }
  }
  Ok(())
}

/// Ensures a basic .gitignore file exists.
fn ensure_gitignore(output_path: &Path) -> Result<()> {
  let gitignore_path = output_path.join(".gitignore");
  if !gitignore_path.exists() {
    info!("Creating basic .gitignore file.");
    // Basic Rust gitignore - enhance later
    let content = "/target\nCargo.lock\n";
    fs::write(gitignore_path, content)?;
  } else {
    info!(".gitignore file already exists.");
    // Future enhancement: check if it contains essential rules like /target
  }
  Ok(())
}

// --- Public Extraction Function ---

/// Extracts a project using the "clean slate" method (copy files, new git history).
pub fn extract_clean_slate(project_id: &str, config: &ProjectConfig) -> Result<ExtractionResult> {
  info!(
    "Starting clean slate extraction for project: {}",
    project_id
  );
  let mut messages = Vec::new();

  // 1. Validate paths
  let source_path = config.internal_repo_path.join(&config.project_subdir);
  if !source_path.exists() {
    return Err(PorterError::PathNotFound(source_path));
  }
  if config.output_path.exists() && fs::read_dir(&config.output_path)?.next().is_some() {
    // Check if directory exists and is not empty
    return Err(PorterError::OutputPathExists(config.output_path.clone()));
  } else if !config.output_path.exists() {
    fs::create_dir_all(&config.output_path)?;
    info!("Created output directory: {}", config.output_path.display());
  }

  // 2. Copy files
  info!(
    "Copying files from {} to {}",
    source_path.display(),
    config.output_path.display()
  );
  let mut copy_options = CopyOptions::new();
  copy_options.content_only = true; // Copy contents of source_path, not the dir itself
  copy_options.overwrite = false; // Should fail if output exists and isn't empty (checked above)
  copy_options.skip_exist = false;

  copy_dir(&source_path, &config.output_path, &copy_options)?;
  messages.push(format!(
    "Copied project files from {}",
    source_path.display()
  ));

  // 3. Initialize Git repo
  info!(
    "Initializing Git repository in {}",
    config.output_path.display()
  );
  run_git_command(&["init"], &config.output_path)?;
  // Optional: Set default branch name if desired (e.g., main)
  // run_git_command(&["branch", "-M", "main"], &config.output_path)?;
  messages.push("Initialized Git repository.".to_string());

  // 4. Add License & .gitignore
  add_license_file(config.license.as_deref(), &config.output_path)?;
  ensure_gitignore(&config.output_path)?;

  // 5. Basic Secret Scan (before commit)
  let secrets_found = scan_secrets_basic(&config.output_path)?;
  if !secrets_found.is_empty() {
    messages.push(format!(
      "Warning: {} potential secrets found during basic scan.",
      secrets_found.len()
    ));
    // Depending on policy, you might want to return Err(PorterError::SecretsFound(...)) here
    // or require a --force flag to proceed. For now, just warn.
  }

  // 6. Initial Git Commit
  info!("Staging files for initial commit.");
  run_git_command(&["add", "."], &config.output_path)?;
  let commit_message = format!("Initial commit of open source project '{}'", project_id);
  info!("Creating initial commit.");
  run_git_command(&["commit", "-m", &commit_message], &config.output_path)?;
  messages.push("Created initial Git commit.".to_string());

  info!(
    "Clean slate extraction completed for project: {}",
    project_id
  );
  Ok(ExtractionResult {
    project_id: project_id.to_string(),
    output_path: config.output_path.clone(),
    messages,
    secrets_found,
  })
}

// --- History Preservation Extraction ---

pub fn extract_preserve_history(
  project_id: &str,
  config: &ProjectConfig,
) -> Result<ExtractionResult> {
  info!(
    "Starting history preservation extraction for project: {}",
    project_id
  );
  let mut messages = Vec::new();

  // 1. Prerequisite Check
  check_tool_exists("git")?; // Ensure git itself exists
  check_tool_exists("git-filter-repo")?;
  messages.push("Checked prerequisites (git, git-filter-repo).".to_string());

  // 2. Validate paths (similar to clean_slate)
  let source_repo_path = &config.internal_repo_path; // Path to the repo root
  let project_subdir_relative = &config.project_subdir; // Relative path within the repo

  if !source_repo_path.join(".git").exists() {
    return Err(PorterError::GitOperation(format!(
      "Internal repo path '{}' does not appear to be a git repository root.",
      source_repo_path.display()
    )));
  }
  if !source_repo_path.join(project_subdir_relative).exists() {
    return Err(PorterError::PathNotFound(
      source_repo_path.join(project_subdir_relative),
    ));
  }
  if config.output_path.exists() && fs::read_dir(&config.output_path)?.next().is_some() {
    return Err(PorterError::OutputPathExists(config.output_path.clone()));
  } else if !config.output_path.exists() {
    fs::create_dir_all(&config.output_path)?;
    info!("Created output directory: {}", config.output_path.display());
  }

  // 3. Create Temporary Clone
  let temp_dir = TempDir::new().map_err(PorterError::TempDir)?;
  let temp_clone_path = temp_dir.path();
  info!(
    "Creating temporary clone of {} in {}",
    source_repo_path.display(),
    temp_clone_path.display()
  );

  // Use file:// protocol for local clones if necessary, adjust if internal repo is remote
  let repo_url = source_repo_path.to_string_lossy(); // Assuming local path for now
  run_git_command(
    &["clone", "--no-local", "--bare", &repo_url, "."],
    temp_clone_path,
  )?; // Use bare clone then checkout? Or full clone? Full clone is simpler.
      // Let's try a full clone first
  run_git_command(&["clone", "--no-local", &repo_url, "."], temp_clone_path)?;
  messages.push(format!(
    "Created temporary clone in {}",
    temp_clone_path.display()
  ));

  // 4. Run git-filter-repo
  info!(
    "Running git-filter-repo for subdir '{}'",
    project_subdir_relative.display()
  );
  let subdir_arg = project_subdir_relative.to_string_lossy(); // Ensure correct format for command arg
                                                              // Use --force because we are operating in a temporary clone
  run_command_capture(
    "git-filter-repo",
    &["--path", &subdir_arg, "--force"],
    temp_clone_path,
  )?;
  messages.push(format!("Ran git-filter-repo on path '{}'", subdir_arg));

  // 5. Move Filtered Repo Contents to Output Path
  info!(
    "Moving filtered repository contents to {}",
    config.output_path.display()
  );
  // We need to move the *contents* of temp_clone_path to output_path
  let mut move_options = CopyOptions::new();
  move_options.content_only = true;
  move_options.overwrite = false; // Should be fine as output_path was empty

  // fs_extra::dir::move_dir requires a callback, even if trivial
  move_dir(temp_clone_path, &config.output_path, &move_options)?;
  messages.push(format!(
    "Moved filtered content to {}",
    config.output_path.display()
  ));

  // TempDir is dropped here, cleaning up the empty temp directory

  // 6. Post-Filtering Checks & Cleanup in Output Repo
  info!(
    "Running post-filtering checks in {}",
    config.output_path.display()
  );

  // 6a. Remove old origin
  match run_git_command(&["remote", "rm", "origin"], &config.output_path) {
    Ok(_) => messages.push("Removed original 'origin' remote.".to_string()),
    Err(e) => warn!("Could not remove 'origin' remote (might not exist): {}", e), // Don't fail if remote doesn't exist
  };

  // 6b. Add License & .gitignore (if they weren't correctly handled by filter-repo or history)
  add_license_file(config.license.as_deref(), &config.output_path)?;
  ensure_gitignore(&config.output_path)?;

  // 6c. Check if license/gitignore were added/modified and need committing
  let git_status = run_git_command(&["status", "--porcelain"], &config.output_path)?;
  let status_output = String::from_utf8_lossy(&git_status.stdout);
  if !status_output.trim().is_empty() {
    info!("Detected changes after filtering (likely license/gitignore), creating cleanup commit.");
    run_git_command(&["add", "LICENSE*", ".gitignore"], &config.output_path)?; // Add specific files
    run_git_command(
      &[
        "commit",
        "-m",
        "chore: Add license and/or gitignore after history filtering",
      ],
      &config.output_path,
    )?;
    messages.push("Created cleanup commit for license/gitignore.".to_string());
  } else {
    info!("No changes detected after filtering, no cleanup commit needed.");
  }

  // 7. Final Secrets Scan (on the resulting code state)
  // Note: This does NOT scan the rewritten history itself.
  let secrets_found = scan_secrets_basic(&config.output_path)?;
  if !secrets_found.is_empty() {
    messages.push(format!(
      "Warning: {} potential secrets found during basic scan of final code state.",
      secrets_found.len()
    ));
  }

  info!(
    "History preservation extraction completed for project: {}",
    project_id
  );
  Ok(ExtractionResult {
    project_id: project_id.to_string(),
    output_path: config.output_path.clone(),
    messages,
    secrets_found, // Only reports secrets in final code state
  })
}
