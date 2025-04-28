# OSS Porter CLI (`oss-porter`)

This directory contains the code for the `oss-porter` command-line interface. This tool helps extract projects from internal Git repositories for open-sourcing and provides an interactive workflow to sync updates afterwards.

## Installation

**(Note: Ensure you have Rust and Cargo installed: [rustup.rs](https://rustup.rs/))**

**From Crates.io (Recommended when published):**
```bash
# Replace 'oss-porter' if the final published name is different
cargo install oss-porter
```

**From Source:**
1. Clone the main repository: `git clone https://github.com/YOUR_USERNAME/oss-porter.git`
2. Navigate to the **project root** directory: `cd oss-porter`
3. Build and install the CLI binary specifically:
   ```bash
   cargo install --path cli
   ```
   This will build the `oss-porter` binary and place it in your `~/.cargo/bin` directory (if it's in your PATH).

Alternatively, to build and run directly for development, navigate to the project root and use:
```bash
cargo run -p oss_porter_cli -- --help
```

## Prerequisites

*   **Git:** The `git` command-line tool must be installed and accessible in your system's PATH.
*   **`git-filter-repo` (Optional):** Required *only* if using the `extract --mode preserve` option. Install via pip (`pip install git-filter-repo`) or your system's package manager.

## Configuration (`~/.oss-porter.toml`)

`oss-porter` relies on a configuration file to define the projects it manages.

*   **Default Location:** `~/.oss-porter.toml` (a hidden file in your home directory).
*   **Override:** Use the global `--config <PATH_TO_FILE>` option before any command to specify a different location.
*   **Initialization:** Run `oss-porter config init` to create a default file at the default location if it doesn't exist.

**File Format (TOML - using `snake_case` keys):**

```toml
# ~/.oss-porter.toml

# Optional global settings table
[settings]
# default_license = "MIT"  # Default license SPDX ID

# Main table containing project definitions
[projects.your-project-id]
# REQUIRED Fields:
internal_repo_path = "/path/to/internal/repo/root" # Absolute path to the Git repository root
project_subdir = "relative/path/to/project"       # Path within the repo (use "." for repo root)
output_path = "/path/for/public/version"          # Absolute path for the extracted/synced public repo clone

# OPTIONAL Fields:
public_repo_url = "git@github.com:your-org/your-repo.git" # Needed for `push`
history_mode = "clean_slate"        # "clean_slate" (default) or "preserve"
license = "Apache-2.0"              # SPDX ID for this project
internal_branch = "main"            # Internal branch for `update` (default: "main")
public_branch = "main"              # Public branch for `push` (default: "main")

[projects.another-project]
# ... other project settings ...
```

**Key Configuration Fields (using `snake_case` in the TOML file):**

*   `internal_repo_path`: **Required**. Absolute path to the Git repository containing the internal project.
*   `project_subdir`: **Required**. Path *relative* to `internal_repo_path` pointing to the directory to extract/sync. Use `.` if the entire repository is the project.
*   `output_path`: **Required**. Absolute path to a directory where `oss-porter` creates/manages the local clone of the public version. Should ideally be empty before first `extract`.
*   `public_repo_url`: Optional. Git URL (SSH/HTTPS) of the public repository. Needed for `push`.
*   `history_mode`: Optional. `"clean_slate"` (default) or `"preserve"`.
*   `license`: Optional. SPDX license identifier (e.g., "MIT"). `extract` adds a placeholder file if missing.
*   `internal_branch`: Optional (defaults to `"main"`). Branch in the internal repo used by `update`.
*   `public_branch`: Optional (defaults to `"main"`). Branch in the public repo used by `push`.

## Command Reference

Run `oss-porter --help` for a list of commands or `oss-porter <COMMAND> --help` for details on a specific command.

*   **`config init`**: Creates the default config file (`~/.oss-porter.toml`).
*   **`config add`**: Interactively adds a new project to the config file.
*   **`config remove <ID>`**: Interactively removes a project from the config.
*   **`config list`**: Lists configured project IDs.
*   **`config show <ID>`**: Displays configuration for one project.
*   **`config validate`**: Checks if the config file can be parsed.
*   **`extract <ID> [--mode <MODE>]`**: Performs the initial extraction (creating `output-path`). `--mode` can be `clean_slate` or `preserve`. **Requires manual review.**
*   **`update <ID>`**: Interactively reviews and applies new commits from the internal repo (`internal-branch`) to the public repo clone (`output-path`). Requires `.oss_porter_state.toml` in the internal subdir. **Requires careful manual review.**
*   **`check <ID>`**: Runs basic checks (dependencies, license, basic secrets) on the project in `output-path`.
*   **`push <ID> [-f|--force]`**: Pushes the `output-path` repository (configured `public_branch`) to the configured `public_repo_url`. `-f` skips confirmation.

## Workflows

These are the primary ways to use `oss-porter`:

### Workflow 1: Initial Project Extraction & Open Sourcing

This workflow takes a project currently only existing inside an internal repository and prepares it for its first public release.

1.  **Initialize Config (if first time):**
    ```bash
    oss-porter config init
    ```

2.  **Define Project:** Add your project's details to `~/.oss-porter.toml`. You can do this manually (using **`snake_case` keys**), or interactively:
    ```bash
    oss-porter config add
    ```
    Follow the prompts to specify paths, branches, history mode, etc.

3.  **Perform Extraction:** Run the extract command, specifying your project ID. Choose the history mode carefully.
    ```bash
    # Using default 'clean-slate' mode
    oss-porter extract your-project-id

    # OR attempting history preservation (requires git-filter-repo)
    oss-porter extract your-project-id --mode preserve
    ```
    This creates the directory specified by `output_path` and populates it with either a clean copy or filtered history, initializing it as a Git repository.

4.  **CRITICAL: Manual Review and Cleanup:** This is the most important step to prevent exposing sensitive information.
    *   Navigate to the output directory: `cd /path/to/your/output-path` (the one specified in the config).
    *   **Inspect Code:** Thoroughly read through the code. Search for:
        *   API keys, passwords, tokens, credentials
        *   Internal hostnames, server names, IP addresses
        *   Confidential comments or internal jargon
        *   Hardcoded paths specific to your internal environment
    *   **Review History (if using `preserve` mode):** This is challenging but essential if you preserved history.
        *   Use `git log`, `git log -p`, `git grep` to search commits.
        *   Consider using external tools like `trufflehog git file://.` to scan the history in the `output-path`.
        *   If sensitive history is found, you may need to use advanced Git commands (`git filter-repo` again, `BFG Repo-Cleaner`) or **consider abandoning history preservation and re-extracting with `clean-slate` mode for safety.**
    *   **Check Dependencies:** Open `Cargo.toml`. Ensure all `[dependencies]`, `[dev-dependencies]`, etc., point to publicly available crates (e.g., from crates.io) or Git URLs. Remove or replace any `path = "..."` dependencies that point to other internal-only projects. `oss-porter check` can help identify these.
    *   **Add/Verify License:** Ensure a `LICENSE` file (e.g., `LICENSE-MIT`, `LICENSE-APACHE`) exists and contains the correct text for your chosen open-source license. Add one if missing. Check your `Cargo.toml` `license` field.
    *   **Write README:** Create or significantly update `README.md`. Explain what the project is, how to build it (`cargo build`), how to run it, how to contribute (if applicable), targeting an external audience.
    *   **Check `.gitignore`:** Ensure it includes standard Rust ignores (`/target`, `Cargo.lock` if it's a library).
    *   **Commit Corrections:** Use standard Git commands within the `output-path` directory to stage and commit any changes made during this review process:
        ```bash
        git add .
        git commit -m "Clean up secrets and prepare for initial release"
        # Or use more granular commits
        ```

5.  **Run Final Checks:**
    ```bash
    oss-porter check your-project-id
    ```
    Review the output for any remaining warnings (basic secrets, path dependencies, missing license). Address them if necessary (go back to step 4).

6.  **Prepare State File (Crucial for Future Updates):**
    *   Identify the commit hash in your **internal repository** that represents the state you just extracted and cleaned. You can find this using `git log` in your internal repo, perhaps filtering by the `project_subdir`.
    *   Create a new file named `.oss_porter_state.toml` *inside* the `project_subdir` within your **internal repository**.
    *   Add the following content to the file, replacing the placeholder hash:
        ```toml
        # .oss_porter_state.toml - Commit this to the internal repo!
        last_synced_internal_commit = "hash_of_internal_commit_at_extract_time"
        ```
    *   Commit this `.oss_porter_state.toml` file to your **internal repository**:
        ```bash
        # Inside the internal repository working directory
        git add path/to/project_subdir/.oss_porter_state.toml
        git commit -m "chore(oss-porter): Add initial sync state for <your-project-id>"
        # Push this internal commit if applicable
        ```

7.  **Push to Public Repository:**
    *   Manually create a new, empty repository on your public Git host (e.g., GitHub, GitLab). **Do not initialize it with a README or license on the host.**
    *   Ensure `public_repo_url` is correctly set in your `~/.oss-porter.toml`.
    *   Run the push command:
        ```bash
        oss-porter push your-project-id
        ```
        Confirm the prompt (unless `-f` is used). This will add the remote `origin` and push your cleaned-up local `output-path` repository (the configured `public_branch`) to the public host.

### Workflow 2: Syncing Updates (Internal Development -> Public Repository)

Use this workflow after the initial release to bring new changes made in the internal repository out to the public one.

1.  **Prerequisites:**
    *   Initial extraction and push must be complete.
    *   The `.oss_porter_state.toml` file must exist and be committed within the internal project subdirectory (`internal_repo_path`/`project_subdir`), containing the correct hash of the last commit successfully synced to the public repo.

2.  **Internal Development:** Make changes and commit them within the internal repository, modifying files inside the `project_subdir`. Ensure these commits are pushed to the `internal_branch` specified in the config (or the default "main").

3.  **Run Interactive Update:**
    ```bash
    oss-porter update your-project-id
    ```

4.  **Review and Apply Commits:**
    *   The tool will fetch updates from the internal repo.
    *   It will then present each new commit (since the last sync) that affected the `project_subdir`.
    *   For each commit, it will display the diff *relative to the subdirectory*.
    *   **Carefully review each diff.** Check for secrets, internal comments, unrelated changes, etc.
    *   Choose an action:
        *   `[Y]es`: Apply this commit to the local public clone (`output-path`) using `git am`.
        *   `[n]o`: Skip this commit permanently for this session. It will likely be shown again on the next `update` run unless the state advances past it.
        *   `[s]kip`: Skip this commit temporarily. It will be shown again later in this session or on the next run.
        *   `[A]ll`: **Use with extreme caution.** Apply all remaining commits non-interactively. This bypasses individual review. Stops immediately if any commit fails to apply (e.g., conflict).
        *   `[q]uit`: Stop the update process now. The state file will be updated to the last successfully applied commit.

5.  **Handle Conflicts:** If `git am` fails due to conflicts during a `[Y]es` or `[A]ll` action:
    *   The tool will report the conflict and stop (or exit 'All' mode).
    *   Manually navigate to the `output_path` directory.
    *   Use `git status`, `git diff`, and your editor to resolve the conflicts in the affected files.
    *   Stage the resolved files: `git add .`
    *   Continue the patch application: `git am --continue` (or abort with `git am --abort`).
    *   Once resolved, re-run `oss-porter update your-project-id`. The tool will read the last successfully applied state and continue processing any remaining commits.

6.  **Commit State Update:** After reviewing all commits (or quitting/conflicting), the tool will show the last internal commit hash that was successfully applied.
    *   It will update the `.oss_porter_state.toml` file in your *internal* repository's working directory.
    *   It will prompt you to commit this state file change to the internal repository. Confirm `[Y]es` (recommended) or `[n]o` (you'll need to commit it manually later).

7.  **CRITICAL: Manual Review and Test:** The update process applies patches, but you still need to verify the result.
    *   Navigate to the `output_path`.
    *   Build the project: `cargo build` (or `cargo build --release`).
    *   Run tests: `cargo test`.
    *   Manually inspect the changes applied, especially if you used the `[A]ll` option.

8.  **Run Checks (Optional):**
    ```bash
    oss-porter check your-project-id
    ```

9.  **Push Updates:** Once satisfied with the state of the `output-path` repository:
    ```bash
    oss-porter push your-project-id
    ```
    Confirm the prompt (unless `-f` is used).