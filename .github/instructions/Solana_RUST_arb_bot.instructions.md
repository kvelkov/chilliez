---
applyTo: '**'
---
# AI Collaboration Guide for the Chilliez Arbitrage Bot

This document provides coding standards, architectural principles, and operational procedures that you, the AI assistant, MUST follow when contributing to this repository. Your primary goal is to write clean, efficient, and maintainable Rust code that aligns with the existing project structure.

## Core Principles

1.  **Primacy of Existing Code:** Before writing ANY new code, you MUST thoroughly search the existing codebase for functions, modules, or utilities that already solve the problem. Your default position should be to reuse and extend existing code, not to create new files or modules. Check the `CODE_MANIFEST.md` first, then the relevant `mod.rs` files.
2.  **Maintainability Over Premature Optimization:** Write clear, readable, and idiomatic Rust. Performance is critical, but it must not come at the cost of code that is difficult to understand or maintain. Do not use `unsafe` code without explicit instruction.
3.  **Ownership and Responsibility:** You are responsible for the entire lifecycle of your changes, from implementation to documentation and testing. A task is not "done" until it is fully documented and all checks pass.

## The Golden Path: Mandatory Development Workflow

You MUST follow these steps in sequence for every task or change request.

### Step 1: Understand the Context & Architecture

* **Consult the Manifest:** Before writing a single line of code, read the `CODE_MANIFEST.md` file to understand the high-level architecture and locate the relevant modules for the task.
* **Explore the Module:** Navigate to the module identified in the manifest. Read its `mod.rs` file and the documentation of its public functions to understand its specific role and conventions.
* **Confirm Your Plan:** Briefly state your implementation plan, confirming which existing files and functions you will modify or extend. **Do not create new files unless no existing module is a logical fit.**

### Step 2: Implement the Changes

* **Follow Idiomatic Rust:** Adhere strictly to modern, idiomatic Rust practices.
* **Error Handling:** All functions that can fail MUST return a `Result`. Use the project's custom `ArbError` enum (`src/error/mod.rs`) for all errors to ensure consistency. **NEVER use `.unwrap()` or `.expect()` in application logic.** Use them only in tests where failure is intended and explicit.
* **Concurrency:** Use the `tokio` runtime and `async/await` syntax for all I/O-bound or concurrent operations, following the patterns established in the `arbitrage/orchestrator` and `websocket` modules.
* **Configuration:** Do not hardcode values. Read all configuration from the unified application `Config` struct (`src/config/settings.rs`).

### Step 3: Document Everything

This is not optional. Every contribution must be documented.

* **Function Documentation:** Add `rustdoc` comments (`///`) to every new public function, struct, and enum you create. The documentation must explain what the function does, its parameters, and what it returns.
* **Module Documentation:** If you make a significant change to a module, update the module-level documentation (`//!`) in the `mod.rs` file to reflect the new functionality.
* **Change Logging:** When you submit your work, provide a concise, bulleted list of the changes you made.

### Step 4: Verify and Test Your Work

No code is considered complete until it passes all the following checks. You must run these commands from the repository root.

1.  **Check for Compile Errors:**
    ```sh
    cargo check
    ```
2.  **Format the Code:**
    ```sh
    cargo fmt
    ```
3.  **Lint for Code Quality (Strict):** Run Clippy and treat all warnings as errors.
    ```sh
    cargo clippy -- -D warnings
    ```
4.  **Run All Tests:** Ensure all unit and integration tests pass.
    ```sh
    cargo test --all-features
    ```

You must report the output of these commands. If any check fails, you must fix the issues and run them again until all checks pass cleanly.

## Architectural Guidelines

* **Single Responsibility Principle:** Keep modules focused. The `dex` module handles DEX interactions, `solana` handles RPC calls, `paper_trading` handles simulation, etc. Do not mix responsibilities.
* **The Orchestrator is the Brain:** The `ArbitrageOrchestrator` (`src/arbitrage/orchestrator/core.rs`) is the central component that coordinates all other services. Major logic flows should be driven from here.
* **Immutability:** Prefer immutable data structures and pass data by reference where possible to avoid unnecessary cloning, especially in performance-sensitive code.

By following these instructions, you will act as a valuable and effective contributor to this project, helping to maintain its high standards of quality and performance.