pub mod check;
pub mod config;
pub mod extract;
pub mod remote;

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
  #[error("I/O Error: {0}")]
  Io(#[from] std::io::Error),
  #[error("Filesystem Extra Error: {0}")]
  FsExtra(#[from] fs_extra::error::Error),
  #[error("Git command failed: {cmd}\nStdout: {stdout}\nStderr: {stderr}")]
  GitCommand {
    cmd: String,
    stdout: String,
    stderr: String,
  },
  #[error("Git operation failed: {0}")]
  GitOperation(String),
  #[error("Project path not found: {0}")]
  PathNotFound(PathBuf),
  #[error("Output path already exists and is not empty: {0}")]
  OutputPathExists(PathBuf),
  #[error("Required tool '{0}' not found in PATH")]
  ToolNotFound(String),
  #[error("Secrets detected: {0}")] // Keep simple initially
  SecretsFound(String),
  #[error("Internal path dependency detected: {0}")]
  InternalDependency(String),
  #[error("TOML parsing error: {0}")]
  TomlParse(#[from] toml::de::Error),
  #[error("TOML serialization error: {0}")]
  TomlSerialize(#[from] toml::ser::Error),
  #[error("Temporary directory error: {0}")]
  TempDir(std::io::Error),
  // Add more specific errors as needed
}

// Define a type alias for Result using our custom error
pub type Result<T> = std::result::Result<T, PorterError>;

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
#[serde(rename_all = "kebab-case")]
pub struct GlobalConfig {
  pub default_license: Option<String>,
  pub secrets_scan_level: Option<String>, // e.g., "none", "basic", "aggressive"
                                          // path_to_trufflehog: Option<PathBuf>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum HistoryMode {
  #[default]
  CleanSlate,
  Preserve,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct ProjectConfig {
  pub internal_repo_path: PathBuf,
  pub project_subdir: PathBuf, // Relative within internal_repo_path
  pub output_path: PathBuf,
  pub public_repo_url: Option<String>,
  #[serde(default)] // Defaults to CleanSlate if missing
  pub history_mode: HistoryMode,
  pub license: Option<String>, // Specific license for this project
                               // Add tags, description etc. later if needed
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
