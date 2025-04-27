use crate::extract::scan_secrets_basic; // Reuse the secrets scan helper
use crate::{CheckResult, PorterError, ProjectConfig, Result}; // Added CheckResult, ProjectConfig
use cargo_toml::{Dependency, Manifest};
use log::{debug, info, warn};
use std::fs;
use std::path::Path; // Needed for path canonicalization

/// Checks a Cargo.toml manifest for path dependencies pointing outside the project directory.
fn check_internal_dependencies(output_path: &Path) -> Result<Vec<String>> {
  info!(
    "Checking for internal path dependencies in {}",
    output_path.display()
  );
  let mut findings = Vec::new();
  let cargo_toml_path = output_path.join("Cargo.toml");

  if !cargo_toml_path.exists() {
    warn!("Cargo.toml not found in output path, skipping dependency check.");
    return Ok(findings);
  }

  let manifest = Manifest::from_path(&cargo_toml_path).map_err(|e| {
    PorterError::Config(format!(
      "Failed to parse {}: {}",
      cargo_toml_path.display(),
      e
    ))
  })?;

  // Canonicalize output path for reliable comparison
  let canonical_output_path = fs::canonicalize(output_path).map_err(|err| PorterError::Io { source: err, path: output_path.to_path_buf() })?;

  let mut check_dep = |name: &str, dep: &Dependency, section: &str| -> Result<()> {
    if let Dependency::Detailed(details) = dep {
      if let Some(dep_path_str) = &details.path {
        debug!(
          "Checking path dependency '{}' from section '[{}]': {}",
          name, section, dep_path_str
        );
        let dep_path = output_path.join(dep_path_str); // Path relative to Cargo.toml
        match fs::canonicalize(&dep_path) {
          Ok(canonical_dep_path) => {
            // Check if the canonical dependency path starts with the canonical output path
            if !canonical_dep_path.starts_with(&canonical_output_path) {
              let finding = format!(
                                "Potential internal path dependency found in section '[{}]': '{}' points to '{}' (outside {})",
                                section, name, dep_path_str, output_path.display()
                            );
              warn!("{}", finding);
              findings.push(finding);
            } else {
              debug!(
                "Dependency '{}' path '{}' is within output directory.",
                name, dep_path_str
              );
            }
          }
          Err(e) => {
            // Path might be invalid, which could also be an issue
            let finding = format!(
                            "Path dependency '{}' in section '[{}]' ('{}') could not be canonicalized: {}. It might be invalid or point outside.",
                            name, section, dep_path_str, e
                        );
            warn!("{}", finding);
            findings.push(finding);
          }
        }
      }
    }
    Ok(())
  };

  // Check different dependency sections
  for (name, dep) in manifest.dependencies {
    check_dep(&name, &dep, "dependencies")?;
  }
  for (name, dep) in manifest.dev_dependencies {
    check_dep(&name, &dep, "dev-dependencies")?;
  }
  for (name, dep) in manifest.build_dependencies {
    check_dep(&name, &dep, "build-dependencies")?;
  }
  // Add checks for target-specific dependencies if needed
  // Add checks for workspace dependencies if needed (more complex)

  info!(
    "Internal dependency check completed. Found {} potential issues.",
    findings.len()
  );
  Ok(findings)
}

/// Runs various checks on the extracted project in the output directory.
pub fn check_project(project_id: &str, config: &ProjectConfig) -> Result<CheckResult> {
  info!(
    "Running checks for project '{}' in {}",
    project_id,
    config.output_path.display()
  );

  if !config.output_path.exists() {
    return Err(PorterError::PathNotFound(config.output_path.clone()));
  }

  let secrets = scan_secrets_basic(&config.output_path)?;
  let internal_deps = check_internal_dependencies(&config.output_path)?;

  // Check for license file existence
  let license_exists = fs::read_dir(&config.output_path)
    .map_err(|err| PorterError::Io { source: err, path: config.output_path.to_path_buf() })?
    .filter_map(|entry| entry.ok())
    .any(|entry| {
      let file_name = entry.file_name().to_string_lossy().to_lowercase();
      file_name.starts_with("license") || file_name.starts_with("copying")
    });
  if !license_exists {
    warn!("No file starting with 'LICENSE' or 'COPYING' found in output directory.");
  }

  Ok(CheckResult {
    project_id: project_id.to_string(),
    secrets_found: secrets,
    internal_deps_found: internal_deps,
    license_ok: license_exists, // Simple check for now
  })
}
