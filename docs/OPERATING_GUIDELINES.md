# 🧠 Solana Arbitrage Bot: Operating Guidelines

This document must be reviewed by all bots, agents, or assistants before each session **and** after completing any major task. These principles are non-negotiable and enforced to ensure project integrity, system continuity, and operational clarity.

---

## 🔍 1. Contextual Awareness & System Observability

*   Before performing **any** task:
    *   Traverse and review the full repository file structure.
    *   Identify interdependencies between modules, services, and helper layers (e.g. `websocket`, `dex`, `arbitrage`, `infra`, `utils`, etc.).
    *   Look for existing implementations of similar logic.
*   **Understand before acting.** Avoid blindly patching or writing redundant logic.

---

## ✅ 2. Functional Consistency

*   **Ensure functions are:**
    *   Properly declared.
    *   Visible across their intended usage scope.
    *   Invoked consistently across the codebase when needed (e.g., shared `calculate_output_amount()`, or `get_price()` APIs).
*   If a helper or shared logic is being duplicated, raise a flag and propose refactoring.

---

## 🗣️ 3. Always Ask When in Doubt

*   If you:
    *   Cannot trace a dependency
    *   Are unsure about Rust module visibility rules
    *   Think a feature might conflict with something else
*   📣 **Ask for clarification. Do not assume.**

---

## 📋 4. Task Workflow & Step Breakdown

*   For every task assigned, you **must**:
    *   Break it into atomic steps
    *   Document each step before execution
    *   List **every affected file and function**
    *   Clarify any external libraries or systems involved (e.g., `.env` config, Solana testnet, DEX clients)

---

## 🧨 5. File Deletion Policy

🚫 **You may not delete any file without explicit written permission.**

*   Temporary, outdated, or unused modules must be confirmed and reviewed.
*   Deletion requests must:
    *   Include justification
    *   Be approved by Kiril (project owner)

---

## ⚠️ 6. Function Disabling or Removal

*   You **must not disable** or comment out any function unless:
    *   A bug requires it for temporary triage (and it’s flagged clearly in the code with a comment)
    *   You’ve received direct permission to remove it

All removed logic must:
*   Be logged
*   Be replaced or explained
*   Be version-tracked

---

## 📌 Summary

This project depends on precision, traceability, and active decision-making. These rules are to be **enforced**, not suggested. Your actions should reflect deep understanding, responsibility, and operational discipline.

Always operate as if the project depends entirely on your foresight—because it does.