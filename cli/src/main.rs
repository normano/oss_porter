use clap::{Parser, Subcommand};
use dialoguer::Confirm;
use oss_porter_core::{
  check::check_project, config::{get_default_config_path, load_config}, extract::{extract_clean_slate, extract_preserve_history}, remote::push_to_remote, ConfigFile, HistoryMode, PorterError, ProjectConfig
};
use std::{fs, path::PathBuf, process::exit}; // For exiting on error

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
  #[arg(short, long, value_name = "FILE", help = "Path to config file")]
  config: Option<PathBuf>,

  #[command(subcommand)]
  command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
  /// Manage configuration
  Config {
    #[command(subcommand)]
    action: ConfigAction,
  },
  /// Extract a project to its public location
  Extract {
    project_id: String,
    #[arg(long, value_enum, help = "Specify history mode (overrides config)")]
    mode: Option<oss_porter_core::HistoryMode>,
  },
  /// Run checks (secrets, dependencies, license) on an extracted project
  Check { project_id: String },
  Push {
    project_id: String,
    #[arg(short, long, help = "Skip confirmation prompt before pushing")]
    force: bool, // Add a force flag to skip prompt
  },
}

#[derive(Subcommand, Debug)]
enum ConfigAction {
  /// Create a default configuration file if one doesn't exist
  Init, // <-- Add Init action
  /// List all configured projects
  List,
  /// Show details for a specific project
  Show { project_id: String },
  /// Validate the configuration file
  Validate,
}

// Default content for the config file
const DEFAULT_CONFIG_CONTENT: &str = r#"# oss-porter Configuration File
# Define global settings and projects to manage.

[settings]
# default_license = "MIT"  # Optional: Set a default license (e.g., "MIT", "Apache-2.0")
# secrets_scan_level = "basic" # Optional: Set default scan level ("none", "basic", "aggressive")

#[projects]
# Example project definition (uncomment and modify):
# [projects.my_cool_library]
# internal_repo_path = "/path/to/your/internal/monorepo_or_project" # REQUIRED: Absolute or relative path to the source Git repo root
# project_subdir = "path/relative/to/repo/root/of/the/project" # REQUIRED: Subdirectory within the repo to extract (use "." if it's the whole repo)
# output_path = "/path/to/where/you/want/the/public_version"    # REQUIRED: Directory where the clean OSS version will be created
# public_repo_url = "git@github.com:your-username/my_cool_library.git" # Optional: URL for the public remote repo
# history_mode = "clean_slate" # Optional: "clean_slate" (default) or "preserve" (requires git-filter-repo)
# license = "MIT" # Optional: License for this specific project (overrides default_license)
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  env_logger::init();
  let cli = Cli::parse();

  // Special handling for 'config init' - doesn't require loading existing config
  if let Commands::Config {
    action: ConfigAction::Init,
  } = cli.command
  {
    return handle_config_init();
  }

  // Load config for all other commands
  let config_file = match load_config(cli.config.as_deref()) {
    Ok(config) => {
      log::info!(
        "Loaded config with {} projects from {}",
        config.projects.len(),
        cli.config.as_deref().map_or_else(
          || get_default_config_path()
            .map_or("default location".to_string(), |p| p.display().to_string()),
          |p| p.display().to_string()
        )
      );
      config
    }
    Err(PorterError::ConfigNotFound(path)) => {
      // Handle specific error
      eprintln!("Error: Configuration file not found at {}", path.display());
      eprintln!("Please run `oss-porter config init` to create a default configuration file,");
      eprintln!("or specify a different path using the --config option.");
      exit(1);
    }
    Err(e) => {
      // Handle other loading errors (parsing, I/O)
      eprintln!("Error loading configuration: {}", e);
      exit(1);
    }
  };

  // Execute the command (excluding init, which was handled above)
  let result = match cli.command {
    Commands::Config { action } => handle_config_action(action, &config_file),
    Commands::Extract { project_id, mode } => handle_extract(project_id, mode, &config_file),
    Commands::Check { project_id } => handle_check(project_id, &config_file),
    Commands::Push { project_id, force } => handle_push(project_id, force, &config_file),
  };

  if let Err(e) = result {
    eprintln!("\nOperation failed: {}", e); // More user-friendly final message
    exit(1);
  }

  Ok(())
}

// Make handlers return a Result to propagate errors
fn handle_config_init() -> Result<(), Box<dyn std::error::Error>> {
  let config_path = get_default_config_path()?;
  // Update the message slightly to reflect the location better if desired
  println!(
    "Checking for configuration file in home directory at: {}",
    config_path.display()
  );

  if config_path.exists() {
    println!("Configuration file already exists. No action taken.");
    return Ok(());
  }

  // Creating parent directory is unlikely needed for home, but harmless
  if let Some(parent_dir) = config_path.parent() {
    // This will likely just be the home directory itself
    if !parent_dir.exists() {
      // This case should be rare unless home dir itself is missing
      fs::create_dir_all(parent_dir).map_err(|e| {
        format!(
          "Failed to access/create config directory '{}': {}",
          parent_dir.display(),
          e
        )
      })?;
      log::info!("Ensured config directory exists: {}", parent_dir.display());
    }
  }

  // Write the default content
  fs::write(&config_path, DEFAULT_CONFIG_CONTENT).map_err(|e| {
    format!(
      "Failed to write default config file '{}': {}",
      config_path.display(),
      e
    )
  })?;

  println!(
    "Successfully created default configuration file at: {}",
    config_path.display()
  );
  println!("Please edit this file to define your projects.");

  Ok(())
}

// Make handlers return a Result to propagate errors
fn handle_config_action(
  action: ConfigAction,
  config: &ConfigFile,
) -> Result<(), Box<dyn std::error::Error>> {
  match action {
    ConfigAction::Init => {
      /* Already handled in main */
      unreachable!()
    }
    ConfigAction::List => {
      println!("Configured Projects:");
      if config.projects.is_empty() {
        println!("  (No projects configured)");
      } else {
        for id in config.projects.keys() {
          println!("- {}", id);
        }
      }
    }
    ConfigAction::Show { project_id } => {
      match config.projects.get(&project_id) {
        Some(proj_config) => println!("{:#?}", proj_config),
        None => {
          eprintln!(
            "Error: Project '{}' not found in configuration.",
            project_id
          );
          // Optionally return an error instead of just printing
        }
      }
    }
    ConfigAction::Validate => {
      // Basic validation happens in load_config, add more if needed
      println!("Configuration loaded successfully.");
      // You could add more specific validation logic here and return Err if needed
    }
  }
  Ok(())
}

// Make handlers return a Result to propagate errors
fn handle_extract(
  project_id: String,
  mode_override: Option<HistoryMode>,
  config_file: &ConfigFile,
) -> Result<(), Box<dyn std::error::Error>> {
  // Return Result
  log::info!("Attempting extraction for project: {}", project_id);

  let project_config = config_file
    .projects
    .get(&project_id)
    .ok_or_else(|| format!("Project '{}' not found in configuration.", project_id))?;

  let history_mode = mode_override.unwrap_or(project_config.history_mode);
  log::info!("Using history mode: {:?}", history_mode);

  // !! Optional: Add a warning/confirmation for preserve mode !!
  if history_mode == HistoryMode::Preserve {
    println!("\nWARNING: History preservation mode ('preserve') uses 'git-filter-repo'.");
    println!(" - This rewrites history and operates on a temporary clone.");
    println!(" - Ensure 'git-filter-repo' is installed and accessible.");
    println!(" - Review the resulting repository carefully for any unintentionally exposed history or secrets.");
    // Add an interactive confirmation prompt here if desired
    // e.g., use a crate like `dialoguer` or simple stdin read.
    // For now, we proceed directly.
  }

  let result = match history_mode {
    HistoryMode::CleanSlate => extract_clean_slate(&project_id, project_config),
    HistoryMode::Preserve => {
      extract_preserve_history(&project_id, project_config) // Calls the newly implemented function
    }
  };

  // ... (rest of the function remains the same - handling Ok/Err) ...
  match result {
    Ok(extraction_result) => {
      println!(
        "\nExtraction successful for project '{}'!",
        extraction_result.project_id
      );
      println!("Mode: {:?}", history_mode); // Indicate mode used
      println!(
        "Output location: {}",
        extraction_result.output_path.display()
      );
      println!("Messages:");
      for msg in extraction_result.messages {
        println!("- {}", msg);
      }
      if !extraction_result.secrets_found.is_empty() {
        println!("\nWARNING: Potential secrets found during scan of FINAL code state:");
        for finding in extraction_result.secrets_found {
          println!("- {}", finding);
        }
        println!(
          "Please review the code AND HISTORY in the output directory carefully before publishing."
        );
      }
    }
    Err(e) => {
      return Err(Box::new(e));
    }
  }
  Ok(())
}

fn handle_check(
  project_id: String,
  config_file: &ConfigFile,
) -> Result<(), Box<dyn std::error::Error>> {
  log::info!("Running checks for project: {}", project_id);

  let project_config = config_file
    .projects
    .get(&project_id)
    .ok_or_else(|| format!("Project '{}' not found in configuration.", project_id))?;

  if !project_config.output_path.exists() {
    eprintln!(
      "Error: Output path '{}' for project '{}' does not exist. Have you extracted it yet?",
      project_config.output_path.display(),
      project_id
    );
    return Ok(()); // Or return an error? Ok for now.
  }

  match check_project(&project_id, project_config) {
    Ok(check_result) => {
      println!("\nCheck Results for project '{}':", check_result.project_id);
      println!("---------------------------------");

      // Secrets
      if check_result.secrets_found.is_empty() {
        println!("[✓] Basic Secret Scan: No obvious secrets found.");
      } else {
        println!(
          "[!] Basic Secret Scan: Found {} potential secrets:",
          check_result.secrets_found.len()
        );
        for finding in check_result.secrets_found {
          println!("  - {}", finding);
        }
      }

      // Internal Dependencies
      if check_result.internal_deps_found.is_empty() {
        println!("[✓] Dependency Check: No path dependencies pointing outside the project found.");
      } else {
        println!(
          "[!] Dependency Check: Found {} potential internal path dependencies:",
          check_result.internal_deps_found.len()
        );
        for finding in check_result.internal_deps_found {
          println!("  - {}", finding);
        }
        println!(
          "    These must be resolved (replaced with public crates or vendored) before publishing."
        );
      }

      // License Check
      if check_result.license_ok {
        println!("[✓] License Check: Found a file starting with 'LICENSE' or 'COPYING'.");
      } else {
        println!("[!] License Check: No file starting with 'LICENSE' or 'COPYING' found.");
        println!("    Ensure you add an appropriate open source license file.");
      }
      println!("---------------------------------");
    }
    Err(e) => {
      // Propagate errors from the check function itself (e.g., path not found, parse errors)
      return Err(Box::new(e));
    }
  }
  Ok(())
}

fn handle_push(
  project_id: String,
  force_prompt: bool, // Renamed from force to force_prompt for clarity
  config_file: &ConfigFile,
) -> Result<(), Box<dyn std::error::Error>> {
  log::info!("Handling push command for project: {}", project_id);

  let project_config = config_file
    .projects
    .get(&project_id)
    .ok_or_else(|| format!("Project '{}' not found in configuration.", project_id))?;

  let public_url = match &project_config.public_repo_url {
    Some(url) => url,
    None => {
      eprintln!(
        "Error: Project '{}' does not have 'public_repo_url' configured. Cannot push.",
        project_id
      );
      return Ok(()); // Exit gracefully, it's a config issue, not a fatal error
    }
  };

  if !project_config.output_path.exists() {
    eprintln!(
      "Error: Output path '{}' for project '{}' does not exist. Cannot push.",
      project_config.output_path.display(),
      project_id
    );
    return Ok(());
  }

  // Confirmation Prompt
  if !force_prompt {
    // Only prompt if --force is NOT used
    let prompt = format!(
      "Are you sure you want to push project '{}' from '{}' to remote '{}'?",
      project_id,
      project_config.output_path.display(),
      public_url
    );

    if !Confirm::new().with_prompt(prompt).interact()? {
      println!("Push cancelled by user.");
      return Ok(());
    }
  } else {
    println!("--force specified, skipping confirmation prompt.");
  }

  // Call the core push function
  println!("Attempting push...");
  match push_to_remote(&project_id, project_config) {
    Ok(()) => {
      println!(
        "\nSuccessfully pushed project '{}' to {}",
        project_id, public_url
      );
    }
    Err(e) => {
      // Propagate errors from the push function (git errors, config errors)
      return Err(Box::new(e));
    }
  }

  Ok(())
}
