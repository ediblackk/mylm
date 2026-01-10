# Compilation Optimization Plan

## Objective
Drastically reduce Rust compilation times (currently 10+ minutes) for the `mylm` project by leveraging faster linkers, build caching, and profile optimizations.

## 1. Tooling Installation & Configuration
**Goal**: Replace standard slow build tools with modern, high-performance alternatives.

*   **Step 1.1**: Install `mold` (Modern Linker).
    *   *Why*: `mold` is 3x-10x faster than the default GNU linker.
    *   *Command*: `sudo apt-get install mold clang` (or equivalent for the distro).
*   **Step 1.2**: Install `sccache` (Shared Compilation Cache).
    *   *Why*: Caches compiled crates globally, preventing recompilation of unmodified dependencies across projects and branch switches.
    *   *Command*: `cargo install sccache` or via package manager.

## 2. Cargo Configuration (`.cargo/config.toml`)
**Goal**: Instruct Cargo to use the new tools.

*   **Step 2.1**: Create/Update `.cargo/config.toml` in the project root.
*   **Configuration**:
    ```toml
    [build]
    # Use sccache to cache build artifacts
    rustc-wrapper = "sccache"

    [target.x86_64-unknown-linux-gnu]
    # Use clang as the linker driver
    linker = "clang"
    # Force use of mold linker
    rustflags = ["-C", "link-arg=-fuse-ld=mold"]
    ```

## 3. Manifest Profile Optimization (`Cargo.toml`)
**Goal**: Reduce the amount of work the compiler needs to do during development.

*   **Step 3.1**: Optimize `[profile.dev]`.
    *   Set `debug = "line-tables-only"` (Reduces debug info size, faster link times).
    *   Set `split-debuginfo = "unpacked"` (Faster linking on Linux).
*   **Step 3.2**: Optimize Dependencies in Dev Mode `[profile.dev.package."*"]`.
    *   Set `opt-level = 3` for dependencies.
        *   *Trade-off*: Slower *initial* compile of dependencies, but much faster runtime for the app during dev, and `sccache` will mitigate the compile time cost after the first run.
    *   Set `debug = false` for dependencies (We rarely debug dependency internals).

## 4. Dependency Feature Pruning
**Goal**: Stop compiling code that isn't used.

*   **Step 4.1**: Audit `tokio`.
    *   Currently using `features = ["full"]`.
    *   *Action*: Switch to granular features (e.g., `["rt-multi-thread", "macros", "net", "signal", "fs", "process"]`).
*   **Step 4.2**: Review `lancedb` and `fastembed`.
    *   Check if default features can be disabled.

## Execution Strategy
1.  **Switch to Code Mode**.
2.  **Run System Checks**: Verify `mold` and `sccache` installation.
3.  **Apply Config Changes**: Write `.cargo/config.toml`.
4.  **Update Manifest**: Edit `Cargo.toml`.
5.  **Benchmark**: Run `cargo clean && cargo build` to measure improvement.