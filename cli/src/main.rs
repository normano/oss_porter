use clap::{Parser, Subcommand};
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};
use log::warn;
use oss_porter_core::{
  check::check_project,
  config::{get_default_config_path, load_config, save_config},
  extract::{extract_clean_slate, extract_preserve_history},
  remote::push_to_remote,
  state::{
    commit_state_file_change, get_internal_state_file_path, read_last_synced_commit,
    write_last_synced_commit, STATE_FILE_NAME,
  },
  update::{
    apply_commit_to_output, get_commit_diff_relative, get_internal_commits_since, ApplyResult,
    CommitInfo,
  },
  ConfigFile, HistoryMode, PorterError, ProjectConfig,
};
use std::{
  fs,
  path::{Path, PathBuf},
  process::exit,
}; // For exiting on error

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
  Check {
    project_id: String,
  },
  Push {
    project_id: String,
    #[arg(short, long, help = "Skip confirmation prompt before pushing")]
    force: bool, // Add a force flag to skip prompt
  },
  Update {
    project_id: String,
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
  /// Add a new project definition (interactively)
  Add,
  /// Remove a project definition
  Remove { project_id: String },
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
# internal_branch = "main" # Default, can be omitted
# public_branch = "main"   # Default, can be omitted
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  env_logger::init();
  let cli = Cli::parse();

  // Special handling for commands that modify config or don't need pre-load
  match &cli.command {
    Commands::Config {
      action: ConfigAction::Init,
    } => {
      return handle_config_init();
    }
    Commands::Config {
      action: ConfigAction::Add,
    } => {
      return handle_config_add_reload(cli.config.as_deref()); // Use _reload version
    }
    Commands::Config {
      action: ConfigAction::Remove { project_id },
    } => {
      return handle_config_remove_reload(project_id.clone(), cli.config.as_deref());
      // Use _reload version
    }
    _ => {} // Other commands proceed to load config
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
    Commands::Config { action } => handle_config_action_read_only(action, &config_file),
    Commands::Extract { project_id, mode } => handle_extract(project_id, mode, &config_file),
    Commands::Check { project_id } => handle_check(project_id, &config_file),
    Commands::Push { project_id, force } => handle_push(project_id, force, &config_file),
    Commands::Update { project_id } => {
      handle_update(project_id, &config_file, cli.config.as_deref())
    }
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
fn handle_config_action_read_only(
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
    ConfigAction::Add => {
      // Need to reload config mutably or pass mutable ref from main
      eprintln!("'config add' requires modification - Refactoring needed in main loop.");
      // Placeholder - Requires adjustment in main's structure
    }
    ConfigAction::Remove { project_id } => {
      // Need to reload config mutably or pass mutable ref from main
      eprintln!("'config remove' requires modification - Refactoring needed in main loop.");
      // Placeholder - Requires adjustment in main's structure
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
    return Err(
      format!(
        "Output path '{}' for project '{}' does not exist. Have you extracted it yet?",
        project_config.output_path.display(),
        project_id
      )
      .into(),
    );
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
    return Err(
      format!(
        "Output path '{}' for project '{}' does not exist. Cannot push.",
        project_config.output_path.display(),
        project_id
      )
      .into(),
    );
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

// Function to check if a path looks like a git repo root
fn is_git_repo(path: &Path) -> bool {
  path.join(".git").is_dir()
}

fn handle_config_add_reload(
  config_path_override: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
  println!("Adding a new project configuration interactively.");

  // Load potentially existing config mutably for validation checks (like existing project IDs)
  // If loading fails (e.g., parse error other than NotFound), report it. Allow NotFound.
  let config_load_result = load_config(config_path_override);
  let mut config_file = match config_load_result {
    Ok(config) => config, // Loaded successfully
    Err(PorterError::ConfigNotFound(_)) => {
      println!("No existing config file found. Creating new one.");
      ConfigFile::default() // Start with an empty config
    }
    Err(e) => {
      eprintln!("Error loading existing configuration: {}", e);
      eprintln!("Cannot proceed with adding project.");
      return Err(e.into()); // Propagate other load errors
    }
  };

  // --- Interactive Input Logic ---
  let theme = ColorfulTheme::default();

  let project_id: String = Input::with_theme(&theme)
    .with_prompt("Project ID (e.g., 'my-library')")
    .validate_with(|input: &String| -> Result<(), String> {
      let trimmed = input.trim();
      if trimmed.is_empty() {
        Err("Project ID cannot be empty.".to_string())
      } else if trimmed.contains(|c: char| c.is_whitespace() || c == '.') {
        Err("Project ID should not contain whitespace or periods.".to_string())
      } else if config_file.projects.contains_key(trimmed) {
        Err(format!(
          "Project ID '{}' already exists in the configuration.",
          trimmed
        ))
      } else {
        Ok(())
      }
    })
    .interact_text()?
    .trim() // Use trimmed version
    .to_string();

  let internal_repo_path_str: String = Input::with_theme(&theme)
    .with_prompt("Internal Git Repo Path (absolute path to the repo root)")
    .validate_with(|input: &String| -> Result<(), String> {
      let path = PathBuf::from(input.trim());
      if !path.is_absolute() {
        Err("Please provide an absolute path.".to_string())
      } else if !is_git_repo(&path) {
        Err(format!(
          "Path '{}' does not seem to contain a .git directory.",
          path.display()
        ))
      } else {
        Ok(())
      }
    })
    .interact_text()?;
  let internal_repo_path = PathBuf::from(internal_repo_path_str.trim());

  let project_subdir_str: String = Input::with_theme(&theme)
    .with_prompt("Project Subdirectory (relative path within the repo, use '.' for repo root)")
    .validate_with(|input: &String| -> Result<(), String> {
      let trimmed_input = input.trim();
      // Allow "." for root
      if trimmed_input == "." {
        return Ok(());
      }
      // Check relative to previously entered repo path
      let path_to_check = internal_repo_path.join(trimmed_input);
      if !path_to_check.exists() {
        Err(format!(
          "Subdirectory '{}' does not exist within '{}'.",
          trimmed_input,
          internal_repo_path.display()
        ))
      } else if !path_to_check.is_dir() {
        Err(format!(
          "Path '{}' within '{}' is not a directory.",
          trimmed_input,
          internal_repo_path.display()
        ))
      } else {
        Ok(())
      }
    })
    .interact_text()?;
  let project_subdir = PathBuf::from(project_subdir_str.trim());

  let output_path_str: String = Input::with_theme(&theme)
    .with_prompt("Output Path (absolute path for the clean OSS version)")
    .validate_with(|input: &String| -> Result<(), String> {
      let path = PathBuf::from(input.trim());
      if !path.is_absolute() {
        Err("Please provide an absolute path.".to_string())
      } else if path.exists() && !path.is_dir() {
        Err(format!(
          "Output path '{}' exists but is not a directory.",
          path.display()
        ))
      } else {
        // Warning if it exists and is not empty can be handled by 'extract'
        Ok(())
      }
    })
    .interact_text()?;
  let output_path = PathBuf::from(output_path_str.trim());

  let history_mode_idx = Select::with_theme(&theme)
    .with_prompt("History Mode")
    .items(&[
      "clean-slate (Recommended Default)",
      "preserve (Requires git-filter-repo)",
    ])
    .default(0)
    .interact()?;
  let history_mode = if history_mode_idx == 0 {
    HistoryMode::CleanSlate
  } else {
    HistoryMode::Preserve
  };

  // --- Branch Configuration Input ---
  let default_branch_name = "main"; // Use a constant for clarity

  let internal_branch: String = Input::with_theme(&theme)
    .with_prompt("Internal Branch to track")
    .default(default_branch_name.to_string()) // Default to "main"
    .validate_with(|input: &String| -> Result<(), &str> {
      if input.trim().is_empty() {
        Err("Branch name cannot be empty.")
      } else {
        Ok(())
      }
    })
    .interact_text()?
    .trim()
    .to_string();

  let public_branch: String = Input::with_theme(&theme)
    .with_prompt("Public Branch to push to")
    .default(default_branch_name.to_string()) // Default to "main"
    .validate_with(|input: &String| -> Result<(), &str> {
      if input.trim().is_empty() {
        Err("Branch name cannot be empty.")
      } else {
        Ok(())
      }
    })
    .interact_text()?
    .trim()
    .to_string();

  // --- Optional Fields Input ---
  let public_repo_url: String = Input::with_theme(&theme)
    .with_prompt("Public Repo URL (Optional, e.g., git@github.com:org/repo.git)")
    .allow_empty(true)
    .interact_text()?;
  let public_repo_url = if public_repo_url.trim().is_empty() {
    None
  } else {
    Some(public_repo_url.trim().to_string())
  };

  let license: String = Input::with_theme(&theme)
    .with_prompt("License (Optional, SPDX ID like 'MIT' or 'Apache-2.0')")
    .allow_empty(true)
    .interact_text()?;
  let license = if license.trim().is_empty() {
    None
  } else {
    Some(license.trim().to_string())
  };

  // --- Construct and Confirm ---
  let new_project = ProjectConfig {
    internal_repo_path,
    project_subdir,
    output_path,
    public_repo_url,
    history_mode,
    internal_branch, // Add new fields
    public_branch,   // Add new fields
    license,
  };

  println!("\n--- New project configuration ---");
  // Use debug print for easy review
  println!("{:#?}", new_project);
  println!("---------------------------------");

  if Confirm::with_theme(&theme)
    .with_prompt("Save this project configuration?")
    .interact()?
  {
    config_file.projects.insert(project_id.clone(), new_project); // Modify the loaded config
    save_config(&config_file, config_path_override)?; // Save the modified config
    println!("Project '{}' added to configuration.", project_id);
  } else {
    println!("Project addition cancelled.");
  }
  Ok(())
}

// --- Materialized Remove Handler ---
fn handle_config_remove_reload(
  project_id: String,
  config_path_override: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
  println!(
    "Attempting to remove project '{}' from configuration.",
    project_id
  );

  // Load config mutably - this time, error out if config doesn't exist
  let mut config_file = match load_config(config_path_override) {
    Ok(config) => config,
    Err(PorterError::ConfigNotFound(path)) => {
      eprintln!(
        "Error: Configuration file not found at {}. Cannot remove project.",
        path.display()
      );
      // Return Ok because the project isn't there to remove anyway? Or Err? Let's Err.
      return Err(PorterError::ConfigNotFound(path).into());
    }
    Err(e) => {
      eprintln!("Error loading configuration: {}", e);
      return Err(e.into());
    }
  };

  // Check if project exists
  if config_file.projects.contains_key(&project_id) {
    println!("\n--- Configuration for project '{}' ---", project_id);
    println!("{:#?}", config_file.projects[&project_id]);
    println!("------------------------------------------");

    // Confirmation Prompt
    if Confirm::with_theme(&ColorfulTheme::default())
      .with_prompt(format!(
        "Are you sure you want to remove project '{}' from the configuration?",
        project_id
      ))
      .default(false) // Default to No for safety
      .interact()?
    {
      config_file.projects.remove(&project_id); // Remove the project
      save_config(&config_file, config_path_override)?; // Save the modified config
      println!("Project '{}' removed from configuration.", project_id);
    } else {
      println!("Removal cancelled.");
    }
  } else {
    eprintln!(
      "Error: Project '{}' not found in the configuration.",
      project_id
    );
    // Return Ok as the desired state (project removed) is already true? Or Err? Let's Err.
    return Err(format!("Project '{}' not found", project_id).into());
  }
  Ok(())
}

fn handle_update(
  project_id: String,
  config_file: &ConfigFile,
  config_path_override: Option<&Path>, // Needed for state commit prompt potentially
) -> Result<(), Box<dyn std::error::Error>> {
  println!("\nStarting interactive update for project: {}", project_id);

  let project_config = config_file
    .projects
    .get(&project_id)
    .ok_or_else(|| format!("Project '{}' not found in configuration.", project_id))?;

  // --- 1. Get Last Synced State ---
  let last_synced_ref = match read_last_synced_commit(project_config)? {
    Some(commit) => commit,
    None => {
      eprintln!(
        "Error: No previous sync state found for project '{}' in the internal repository.",
        project_id
      );
      eprintln!("       Please ensure '{}' exists within '{}' and contains the hash of the last commit synced.",
                     STATE_FILE_NAME, project_config.internal_repo_path.join(&project_config.project_subdir).display());
      eprintln!(
        "       If this is the first sync after an initial extract, manually create the state file"
      );
      eprintln!("       with the initial commit hash from the internal repo that corresponds to the extract point.");
      // Alternatively, could prompt user for the initial hash here.
      return Err("Missing initial sync state.".into()); // Use Box<dyn Error> for simple errors
    }
  };
  println!("Last synced internal commit: {}", last_synced_ref);

  // --- 2. Identify New Commits ---
  let mut commits_to_review = get_internal_commits_since(project_config, Some(&last_synced_ref))?;

  if commits_to_review.is_empty() {
    println!(
      "Project is up-to-date. No new commits found since {}.",
      last_synced_ref
    );
    return Ok(());
  }
  println!(
    "Found {} new candidate commits to review.",
    commits_to_review.len()
  );

  // --- 3. Interactive Review Loop ---
  let mut successfully_applied_commit: Option<String> = Some(last_synced_ref.clone()); // Track last successful apply
  let mut apply_all_mode = false;
  let mut user_quit = false;
  let mut skipped_commits: Vec<CommitInfo> = Vec::new(); // Track explicitly skipped ('n')

  while let Some(commit_info) = commits_to_review.pop_front() {
    // Process oldest first
    let current_commit_hash = commit_info.hash.clone();
    println!("\n--- Reviewing Commit: {} ---", current_commit_hash);
    println!("Subject: {}", commit_info.subject);

    let choice: usize;

    if !apply_all_mode {
      // Show Diff
      match get_commit_diff_relative(project_config, &current_commit_hash) {
        Ok(diff) => {
          // Simple print, consider paging or better display for large diffs
          println!("{}", diff);
          // Check if diff is empty - might indicate changes outside subdir pathspec logic?
          if diff.trim().is_empty() {
            warn!("Commit {} produced an empty diff relative to '{}'. Check pathspec logic or commit content.",
                                  current_commit_hash, project_config.project_subdir.display());
          }
        }
        Err(e) => {
          eprintln!(
            "Error getting diff for commit {}: {}",
            current_commit_hash, e
          );
          // Offer to skip or quit?
          if Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Failed to get diff. Skip this commit?")
            .interact()?
          {
            skipped_commits.push(commit_info); // Treat as skipped ('n')
            continue;
          } else {
            user_quit = true;
            break;
          }
        }
      }

      // Prompt User
      choice = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(format!(
          "Apply commit {} to '{}'?",
          current_commit_hash,
          project_config.output_path.display()
        ))
        .items(&[
          "Yes",                 // 0
          "No (skip always)",    // 1
          "Skip for now",        // 2
          "Apply ALL remaining", // 3
          "Quit update",         // 4
        ])
        .default(0)
        .interact()?;
    } else {
      // In Apply All mode, implicitly choose Yes
      println!("Applying non-interactively (Apply All mode)...");
      choice = 0; // Simulate "Yes"
    }

    match choice {
      // --- Yes ---
      0 => {
        match apply_commit_to_output(project_config, &current_commit_hash)? {
          // Calls the new patch-based function
          ApplyResult::Success => {
            successfully_applied_commit = Some(current_commit_hash.to_string()); // Update latest success
          }
          ApplyResult::Conflict => {
            eprintln!(
              "\nError: Patch application conflict detected for commit {}.",
              current_commit_hash
            );
            eprintln!("The 'git am' command failed. Please resolve the conflicts manually in the output directory:");
            eprintln!("  cd {}", project_config.output_path.display());
            eprintln!(
              "  # (Review conflicts with 'git status', 'git diff', edit files, 'git add .')"
            );
            eprintln!("  git am --continue"); // Updated instruction
            eprintln!(
              "Once resolved, re-run 'oss-porter update {}' to process remaining commits.",
              project_id
            );
            eprintln!("To abort the conflicting patch application: git am --abort"); // Updated instruction
            user_quit = true; // Force quit after conflict
            break; // Exit review loop
          }
          ApplyResult::Failure(stderr) => {
            eprintln!(
              "\nError: Failed to apply patch for commit {} (non-conflict error):",
              current_commit_hash
            );
            eprintln!("{}", stderr);
            eprintln!("Update process aborted. The failed 'git am' session may have been automatically aborted.");
            user_quit = true;
            break; // Exit review loop
          }
        }
      }
      // --- No (skip always) ---
      1 => {
        println!(
          "Skipping commit {} permanently for this session.",
          current_commit_hash
        );
        skipped_commits.push(commit_info);
        // Do NOT update successfully_applied_commit beyond the previous one
      }
      // --- Skip for now ---
      2 => {
        println!("Skipping commit {} for now.", current_commit_hash);
        commits_to_review.push_back(commit_info); // Put it at the end of the queue
                                                  // Do NOT update successfully_applied_commit
      }
      // --- Apply ALL remaining ---
      3 => {
        println!("Entering non-interactive 'Apply All' mode...");
        apply_all_mode = true;
        // Re-add the current commit to the front to apply it first in 'All' mode
        commits_to_review.push_front(commit_info);
      }
      // --- Quit update ---
      4 => {
        println!("Quitting update process as requested.");
        user_quit = true;
        break; // Exit review loop
      }
      _ => unreachable!(),
    }

    // If we hit a conflict or failure in 'Apply All' mode, break immediately
    if apply_all_mode
      && choice == 0
      && successfully_applied_commit != Some(current_commit_hash.to_string())
    {
      // Check if the last apply action wasn't successful (conflict or failure occurred)
      println!("Stopping 'Apply All' mode due to conflict or failure.");
      user_quit = true; // Treat as quit to save state correctly
      break;
    }
  } // End while loop

  // --- 4. Completion ---
  println!("\n---------------------------------");
  if user_quit {
    println!("Update process exited or was aborted.");
  } else if apply_all_mode {
    println!("Update process finished (Apply All mode completed).");
    println!("[WARN] Commits were applied non-interactively. Please review changes carefully.");
  } else {
    println!("Update process finished reviewing commits.");
  }

  if !skipped_commits.is_empty() {
    println!("Explicitly skipped commits (will need review on next run):");
    for skipped in skipped_commits {
      println!(" - {} {}", skipped.hash, skipped.subject);
    }
  }

  // Save the state corresponding to the *last successfully applied* commit
  let final_synced_commit = successfully_applied_commit.as_deref();
  println!(
    "Last successfully synced internal commit is now: {}",
    final_synced_commit.unwrap_or("<none - state cleared or no commits applied>")
  );

  // Write state file (non-optional, always record last success)
  write_last_synced_commit(project_config, final_synced_commit)?;

  // Prompt to commit state file change
  if Confirm::with_theme(&ColorfulTheme::default())
    .with_prompt(format!(
      "Commit this sync state update ({}) to the internal repository '{}'?",
      final_synced_commit.unwrap_or("<none>"),
      project_config.internal_repo_path.display()
    ))
    .interact()?
  {
    match commit_state_file_change(project_config, final_synced_commit) {
      Ok(()) => println!("State file committed successfully."),
      Err(e) => eprintln!("Error committing state file to internal repo: {}", e), // Don't fail entire command for this
    }
  } else {
    println!("Skipped committing state file update to internal repository.");
    println!(
      "Reminder: Commit the change in '{}' manually.",
      get_internal_state_file_path(project_config).display()
    );
  }

  println!("\nUpdate interaction complete.");
  if !user_quit {
    // Only give next steps if user didn't explicitly quit midway
    println!(
      "Please review changes in '{}', build/test, and run checks.",
      project_config.output_path.display()
    );
    println!(
      "When ready, push changes using 'oss-porter push {}' or git.",
      project_id
    );
  }

  Ok(())
}
