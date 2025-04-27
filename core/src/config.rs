use crate::{ConfigFile, PorterError, Result};
use directories::ProjectDirs;
use std::{
  fs,
  path::{Path, PathBuf},
};

// Helper function to get the expected config file path
pub fn get_default_config_path() -> Result<PathBuf> {
  // Qualifiers reversed for standard directory structure (org.domain.AppName)
  if let Some(proj_dirs) = ProjectDirs::from("com", "oss-porter", "oss-porter") {
    // Adjust qualifiers if desired
    let config_dir = proj_dirs.config_dir();
    Ok(config_dir.join("config.toml"))
  } else {
    Err(PorterError::Config(
      "Could not determine standard configuration directory.".to_string(),
    ))
  }
}

// Modify load_config to use the default path unless overridden
pub fn load_config(path_override: Option<&Path>) -> Result<ConfigFile> {
  let config_path = match path_override {
    Some(p) => p.to_path_buf(),
    None => get_default_config_path()?, // Use the default path finder
  };

  log::debug!(
    "Attempting to load configuration from: {}",
    config_path.display()
  );

  match fs::read_to_string(&config_path) {
    Ok(content) => {
      let config: ConfigFile = toml::from_str(&content).map_err(|e| {
        PorterError::Config(format!(
          "Failed to parse config file '{}': {}",
          config_path.display(),
          e
        ))
      })?;
      // Add validation logic here if needed (e.g., check paths exist AFTER parsing)
      Ok(config)
    }
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
      // Specific error for not found case
      Err(PorterError::ConfigNotFound(config_path))
    }
    Err(e) => {
      // Other I/O errors
      Err(PorterError::Io(e))
    }
  }
}
// Add function to save config later if needed for `config add/remove`
