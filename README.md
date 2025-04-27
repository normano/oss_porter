# OSS Porter

[![crates.io](https://img.shields.io/crates/v/oss-porter.svg)](https://crates.io/crates/oss-porter) <!-- Placeholder: Update when published -->
[![Build Status](https://github.com/YOUR_USERNAME/oss-porter/actions/workflows/rust.yml/badge.svg)](https://github.com/YOUR_USERNAME/oss-porter/actions) <!-- Placeholder: Update CI badge URL -->
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](https://opensource.org/licenses/MIT) <!-- Placeholder: Choose license -->

`oss-porter` helps safely extract projects from internal Git repositories into public ones and syncs updates interactively. It aims to streamline open-sourcing while emphasizing manual review to prevent leaking sensitive data.

## Features

*   TOML configuration (`~/.oss-porter.toml`) for managing multiple projects.
*   `extract` command with `clean_slate` (default, no history) or `preserve` (attempts history filtering via `git-filter-repo`) modes.
*   `update` command for interactive, commit-by-commit synchronization from internal to public repos.
*   Internal state tracking (`.oss_porter_state.toml` committed to the internal repo) for reliable updates.
*   Basic checks (`check` command) for path dependencies and license files.
*   Push helper (`push` command) to deploy to the public remote.

## Prerequisites

*   Rust Toolchain ([rustup.rs](https://rustup.rs/))
*   Git
*   `git-filter-repo` (Optional, only for `extract --mode preserve`)

## Installation

**From Crates.io (Recommended):**
```bash
cargo install oss-porter # Or final published name
```

**From Source:**
```bash
git clone https://github.com/YOUR_USERNAME/oss-porter.git
cd oss-porter
cargo install --path cli
```

## Quick Start & Configuration

1.  **Initialize:** Run `oss-porter config init` to create `~/.oss-porter.toml`.
2.  **Configure:** Edit `~/.oss-porter.toml`. Define projects under `[projects.your-id]` using `snake_case` keys like `internal_repo_path`, `project_subdir`, `output_path`, etc. See comments in the generated file or use `oss-porter config add` for interactive setup.

## Core Commands

*   `config init|add|remove|list|show|validate`: Manage configuration.
*   `extract <ID> [--mode clean|preserve]`: Initial project extraction. **Manual review required!**
*   `update <ID>`: Interactively sync internal changes. **Manual review required!**
*   `check <ID>`: Run basic checks on the output directory.
*   `push <ID> [-f|--force]`: Push output directory to public remote.

Use `oss-porter <COMMAND> --help` for details.

## Workflows

### 1. Initial Extraction

1.  `config init` & `config add` (or edit `~/.oss-porter.toml`).
2.  `extract <ID>` (select mode).
3.  **CRITICAL: Manually Review & Clean** the code/history/dependencies/license/README in the `output_path`. Commit fixes within `output_path`.
4.  `check <ID>`.
5.  Create empty public remote repo.
6.  Create & commit `.oss_porter_state.toml` (with internal extract commit hash) inside the *internal* repo's `project_subdir`.
7.  `push <ID>`.

### 2. Syncing Updates

1.  Ensure initial setup & state file exist internally.
2.  Make/commit changes internally.
3.  `update <ID>` (Follow interactive prompts, review diffs carefully).
4.  Resolve conflicts manually in `output_path` if they occur, then re-run `update <ID>`.
5.  Confirm prompt to commit the updated `.oss_porter_state.toml` internally.
6.  **CRITICAL: Manually Review & Test** the changes in `output_path`.
7.  `check <ID>` (Optional).
8.  `push <ID>`.

## Development

This is a Cargo workspace: `core/` (library), `cli/` (binary).

```bash
# Build
cargo build
# Run CLI
cargo run -p oss-porter-cli -- <COMMAND> [ARGS...]
```