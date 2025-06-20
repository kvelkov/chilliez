---
applyTo: '**'
---
# AI Collaboration Guide for the Chilliez Arbitrage Bot

This document defines coding standards and procedures that **must** be followed. Your main objective is to write clean, efficient, maintainable **Rust** code that integrates with the existing architecture.

## Core Principles

1. **Reuse First:** Before coding, search for existing functionality. Start with `CODE_MANIFEST.md` and check `mod.rs` files. Avoid adding new files unless absolutely necessary.
2. **Clarity Over Over-Optimization:** Write idiomatic, readable Rust. Avoid `unsafe` unless instructed.
3. **End-to-End Responsibility:** Implement, document, and test your work. A task isn’t complete until all checks pass.

## Mandatory Workflow

### 1. Understand the Architecture

- **Read `CODE_MANIFEST.md`** to locate relevant modules.
- **Inspect `mod.rs`** and public APIs of the target module.
- **Confirm Plan:** State which functions/modules you'll use or update. Avoid creating new files unless needed.

### 2. Implement the Change

- **Idiomatic Rust only.**
- **Errors:** Use `Result` and the centralized `ArbError` (`src/error/mod.rs`). Never use `.unwrap()` or `.expect()` in app logic (tests are OK).
- **Concurrency:** Use `tokio` and `async/await` for async operations, modeled after `orchestrator` and `websocket`.
- **Config:** No hardcoded values. Use the global `Config` struct (`src/config/settings.rs`).

### 3. Documentation

**Required.**

- **Function-level:** Use `///` doc comments for all public functions, structs, enums.
- **Module-level:** Update `//!` in `mod.rs` for major changes.
- **Change Summary:** Submit a bulleted list of your changes.

### 4. Verify and Test

Run these from the project root. All must pass:

```sh
cargo check
cargo fmt
cargo clippy -- -D warnings
cargo test --all-features