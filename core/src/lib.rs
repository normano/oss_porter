pub mod check;
pub mod config;
pub mod extract;
pub mod remote;
pub mod state;
pub mod update;
pub mod utils;

use std::path::PathBuf;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PorterError {
  #[error("Configuration Error: {0}")]
  Config(String),
  #[error("Configuration file not found at path: {0}\nConsider running `oss-porter config init` to create a default file.")]
  ConfigNotFound(PathBuf),
  #[error("I/O Error accessing '{path}': {source}")] // Add path context
  Io {
    #[source]
    source: std::io::Error,
    path: PathBuf,
  },
  #[error("Filesystem operation failed: {0}")] // Keep FsExtra general
  FsExtra(#[from] fs_extra::error::Error),
  #[error("Git command failed.\n Command: {cmd}\n CWD: {cwd}\n Status: {status}\n Stdout: {stdout}\n Stderr: {stderr}")]
  GitCommand {
    // Add CWD and status
    cmd: String,
    cwd: PathBuf,   // Add working directory context
    status: String, // Add exit status
    stdout: String,
    stderr: String,
  },
  #[error("Git operation failed: {0}")]
  GitOperation(String),
  #[error("Required path not found: {0}")] // Slightly clearer than 'Project path'
  PathNotFound(PathBuf),
  #[error("Output path already exists and is not empty: {0}")]
  OutputPathExists(PathBuf),
  #[error("Required external tool '{0}' not found in PATH. Please install it.")] // Add hint
  ToolNotFound(String),
  #[error("Secrets Scan Warning: {0}")] // Rephrase as warning? Or keep as error? TBD
  SecretsFound(String), // Maybe change this later based on policy
  #[error("Dependency Check Warning: {0}")] // Rephrase as warning
  InternalDependency(String), // Maybe change this later
  #[error("Failed to parse TOML file '{path}': {source}")] // Add path context
  TomlParse {
    #[source]
    source: toml::de::Error,
    path: PathBuf,
  },
  #[error("Failed to serialize TOML data: {0}")] // Serialization usually isn't path specific
  TomlSerialize(#[from] toml::ser::Error),
  #[error("Failed to create/access temporary directory: {source}")] // Specific source
  TempDir {
    #[source]
    source: std::io::Error,
  },
}

// Define a type alias for Result using our custom error
pub type Result<T> = std::result::Result<T, PorterError>;

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct GlobalConfig {
  pub default_license: Option<String>,
  pub secrets_scan_level: Option<String>, // e.g., "none", "basic", "aggressive"
                                          // path_to_trufflehog: Option<PathBuf>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum HistoryMode {
  #[default]
  CleanSlate,
  Preserve,
}

// Helper function for default branch name
fn default_branch() -> String {
  "main".to_string()
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ProjectConfig {
  pub internal_repo_path: PathBuf,
  pub project_subdir: PathBuf, // Relative within internal_repo_path
  pub output_path: PathBuf,
  pub public_repo_url: Option<String>,
  #[serde(default)] // Defaults to CleanSlate if missing
  pub history_mode: HistoryMode,
  pub license: Option<String>, // Specific license for this project
                               // Add tags, description etc. later if needed
  // New Branch Configuration
  #[serde(default = "default_branch")] // Use helper for default value "main"
  pub internal_branch: String, // Branch to track in the internal repo for updates

  #[serde(default = "default_branch")] // Use helper for default value "main"
  pub public_branch: String,   // Branch to push to in the public repo
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct ConfigFile {
  #[serde(default)]
  pub settings: GlobalConfig,
  #[serde(default)]
  pub projects: HashMap<String, ProjectConfig>,
}

#[derive(Debug)]
pub struct ExtractionResult {
  pub project_id: String,
  pub output_path: PathBuf,
  pub messages: Vec<String>, // Log messages or warnings during extraction
  pub secrets_found: Vec<String>, // List of potential secrets found
}

#[derive(Debug)]
pub struct CheckResult {
  pub project_id: String,
  pub secrets_found: Vec<String>,
  pub internal_deps_found: Vec<String>,
  pub license_ok: bool,
  // Add other check results
}
