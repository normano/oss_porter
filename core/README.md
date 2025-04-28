# OSS Porter Core Library (`oss_porter_core`)

This crate provides the foundational logic and functionality for the `oss-porter` toolset. It handles configuration management, Git interactions, project extraction, state tracking, update processing, and basic checks required for managing the synchronization between internal and public project repositories.

## Purpose

This library is primarily consumed by the `oss_porter_cli` binary crate and potentially other frontends (like a GUI). It encapsulates the core operations in a reusable way. While it can be used directly in other Rust applications needing similar functionality, its API is mainly tailored to the needs of `oss-porter`.

## Modules

The core logic is organized into several modules:

*   **`config`**: Handles loading, parsing, saving, and finding the `oss-porter.toml` configuration file. Defines the `ConfigFile` and `ProjectConfig` structs.
*   **`state`**: Manages the synchronization state (`.oss_porter_state.toml`), including reading the last synced commit, writing updates, and committing state changes back to the internal repository.
*   **`extract`**: Implements the initial project extraction logic for both `clean_slate` (file copy) and `preserve` (history filtering via `git-filter-repo`) modes. Includes logic to exclude the state file from the extracted output.
*   **`update`**: Contains the functions supporting the interactive update workflow:
    *   `get_internal_commits_since`: Finds relevant new commits in the internal repository.
    *   `get_commit_diff_relative`: Generates formatted diffs for review.
    *   `apply_commit_to_output`: Applies changes from a specific internal commit to the output repository using a patch-based approach (`git format-patch` + `git am`).
*   **`check`**: Provides functions to perform basic checks on the output repository, such as looking for internal path dependencies in `Cargo.toml` and checking for license file presence.
*   **`remote`**: Handles interactions with the public Git remote, specifically the `push_to_remote` functionality.
*   **`utils`**: Contains helper functions, primarily for executing external commands like `git` and `git-filter-repo` reliably and capturing their output.
*   **`lib.rs`**: Defines the top-level structs (`ProjectConfig`, `ConfigFile`, etc.) and the main `PorterError` enum using `thiserror`.

## Key Structs & Enums

*   **`ProjectConfig`**: Represents the configuration specific to a single project being managed (paths, branches, mode, etc.). Defined in `lib.rs`. **Note:** Expects `snake_case` keys when deserialized from TOML.
*   **`ConfigFile`**: Represents the entire parsed `oss-porter.toml` file, including global settings and the map of projects. Defined in `lib.rs`.
*   **`PorterError`**: The central error type for all fallible operations within the core library. Defined in `lib.rs`.
*   **`HistoryMode`**: Enum for extraction modes (`CleanSlate`, `Preserve`). Defined in `lib.rs`.
*   **`CommitInfo`**: Simple struct holding commit hash and subject, used by the update module. Defined in `update.rs`.
*   **`ApplyResult`**: Enum indicating the outcome of attempting to apply a commit/patch (`Success`, `Conflict`, `Failure`). Defined in `update.rs`.
*   **`ExtractionResult` / `CheckResult`**: Structs holding the results of their respective operations. Defined in `lib.rs`.

## Direct Usage Example

While primarily used by the CLI, you could use the core library like this:

```rust
use oss_porter_core::{
    config::{self, ConfigFile},
    ProjectConfig, PorterError, Result,
    // Import specific modules as needed, e.g.:
    // extract, update, state, check, remote
};
use std::path::Path;

fn example_core_usage(config_path: Option<&Path>, project_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Load Configuration
    let config_file: ConfigFile = config::load_config(config_path)?;
    let project_config: &ProjectConfig = config_file.projects.get(project_id)
        .ok_or_else(|| format!("Project '{}' not found in configuration", project_id))?;

    // 2. Example: Read Sync State
    // let last_sync = oss_porter_core::state::read_last_synced_commit(project_config)?;
    // println!("Last sync: {:?}", last_sync);

    // 3. Example: Perform Checks
    // let check_result = oss_porter_core::check::check_project(project_id, project_config)?;
    // println!("Check Results: {:?}", check_result);
    // if !check_result.internal_deps_found.is_empty() {
    //     println!("Warning: Found internal dependencies!");
    // }

    // 4. Example: Perform Extraction (handle Result)
    // oss_porter_core::extract::extract_clean_slate(project_id, project_config)?;

    // 5. Example: Trigger Update Logic (more complex interaction needed)
    // let commits = oss_porter_core::update::get_internal_commits_since(project_config, last_sync.as_deref())?;
    // for commit in commits {
    //     // Get diff, prompt user (external logic), apply if confirmed...
    // }

    println!("Example usage finished for project '{}'", project_id);
    Ok(())
}
```