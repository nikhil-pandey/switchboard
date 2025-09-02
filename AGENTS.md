## 🧑‍💻 Engineering Rules

### 🦀 Rust

* **Edition:** Use the latest stable edition (`2024` at present).
* **Dependency Management:**

  * **NEVER** edit `Cargo.toml` by hand. Always add dependencies with:

    ```sh
    cargo add <crate>
    ```
  * If part of a workspace, centralize shared versions in the root `Cargo.toml` under `[workspace.dependencies]`, and depend on them from member crates with `{ workspace = true }`. Do not pin different versions in member crates.
* **Toolchain:**

  * Use `rustup` for all toolchain management.
  * Test, format, and lint with:

    ```sh
    cargo test
    cargo fmt -- --check
    cargo clippy --all-targets --all-features -- -D warnings
    ```
* **Commit Policy:**

  * No merges without passing tests, formatting, and clippy lints.

#### Breaking changes that you should be aware of
* Setting environment variables is now considered unsafe and should be avoided. If you need to, use `unsafe { ... }` block to set them.
* Avoid using `unsafe` blocks unless absolutely necessary.

#### Workspace dependency policy (must-follow)

- Centralize versions in the root Cargo.toml under `[workspace.dependencies]`.
- Member crates depend on shared crates via `{ workspace = true }`.
- Only pin a version locally when the dependency is unique to that crate. Document why.
- When bumping a dependency, bump in one place (root), build the workspace, then fix breakages at call sites.

Examples:
- Good: `sysinfo = { workspace = true }` in crates that use it; root sets `sysinfo = "…"`.
- Bad: `sysinfo = "…"` scattered across multiple crate Cargo.toml files.

---

### 🚦 General Coding Guidelines

* **Principles:**

  * Follow **DRY** (Don’t Repeat Yourself) and **SOLID** principles.
  * Clarity and maintainability > speed or “hacks.” If in doubt, ask.
  * Prefer small, composable functions. Avoid long monolithic methods: extract IO, parsing, validation and business logic into helpers.
  * Do not use deprecated APIs when a supported alternative exists. Upgrade to the latest recommended calls (e.g., `aws_config::load_defaults` + `BehaviorVersion`).
  * Stream large payloads to disk instead of buffering in memory; enforce explicit limits up front.

* **Commit Quality:**

  * Commits must be atomic, with clear, descriptive messages.
  * Avoid WIP or “fix” commits; squash where appropriate.

* **Secrets:**

  * **Never** commit secrets, keys, or tokens to the repository.
  * Use environment variables or secure secrets management only.

---

### 📚 Documentation & Collaboration

* **Docs:**

  * All public APIs and important modules must have doc comments or markdown docs.
  * Update README, usage, and setup instructions with all changes.

* **Tests:**

  * Code changes must be accompanied by meaningful tests.
  * Aim for high coverage, but never sacrifice clarity for coverage metrics.

---

### 🛑 What NOT to Do

* Never modify dependency files or manifests directly—**always** use the prescribed tool.
* Never bypass code quality tools (formatters, linters, tests) or ignore warnings.
* Never push code that “just works for me.”
* Never leave TODOs or commented code in merged code.
* Always follow these instructions **exactly**.
* If unsure how to proceed, prefer to ask/stop rather than guess.
* Use standard logging and error-handling conventions.
* Do not write useless comments. Same goes for naming.
* Always follow DRY (Don't Repeat Yourself) and SOLID (Single Responsibility Principle) principles and clean architecture.

### Project stability policy (early stage)

- This project is in an early stage; breaking changes across crates and public APIs are acceptable when they improve correctness and architecture. We will not hold back necessary refactors due to fear of breaking.
- We do not maintain a changelog yet. Update AGENTS.md and crate READMEs with any significant behavior changes as needed.
