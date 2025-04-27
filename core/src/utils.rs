// oss-porter-core/src/utils.rs
use crate::{PorterError, Result};
use log::{debug, error};
use std::{
  path::{Path, PathBuf},
  process::{Command, Output},
};

/// Runs a command in the specified directory, capturing output.
pub fn run_command_capture(cmd_name: &str, args: &[&str], cwd: &Path) -> Result<Output> {
  let cmd_str = format!("{} {}", cmd_name, args.join(" "));
  debug!(
    "Running command: '{}' in directory: {}",
    cmd_str,
    cwd.display()
  );

  let output = Command::new(cmd_name)
    .args(args)
    .current_dir(cwd)
    .output()
    .map_err(|e| PorterError::Io {
      source: e,
      path: cwd.to_path_buf(), // Path where command was run
    })?;

  if !output.status.success() {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    error!("Command failed: {}", cmd_str);
    error!("Stderr: {}", stderr);
    error!("Stdout: {}", stdout);
    return Err(PorterError::GitCommand {
      // Using GitCommand for general command errors now
      cmd: cmd_str,
      cwd: cwd.to_path_buf(),
      status: output.status.to_string(),
      stdout,
      stderr,
    });
  }
  debug!("Command successful: {}", cmd_str);
  Ok(output)
}

/// Runs a git command specifically.
pub fn run_git_command(args: &[&str], cwd: &Path) -> Result<Output> {
  // Could add check_tool_exists("git") here if desired
  run_command_capture("git", args, cwd)
}

// Add check_tool_exists if needed by other modules outside extract.rs
pub fn check_tool_exists(tool_name: &str) -> Result<()> {
  use std::process::Stdio;
  Command::new(tool_name)
    .arg("--version") // Most tools support --version or similar
    .stdout(Stdio::null()) // Don't capture output unless needed for debugging
    .stderr(Stdio::null())
    .status()
    .map_err(|e| {
      if e.kind() == std::io::ErrorKind::NotFound {
        PorterError::ToolNotFound(tool_name.to_string())
      } else {
        // Treat other errors (like permission denied) as general I/O errors
        // related to trying to execute the tool. Using PathBuf::new() as placeholder.
        PorterError::Io {
          source: e,
          path: PathBuf::from(tool_name),
        }
      }
    })? // Check if the command even starts
    .success() // Check if the command ran successfully (exit code 0)
    .then_some(()) // Convert success to Ok(())
    .ok_or_else(|| PorterError::ToolNotFound(format!("Tool '{}' command check failed.", tool_name)))
}
