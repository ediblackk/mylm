# Contributing to mylm

First off, thank you for considering contributing to `mylm`! It's people like you that make the open source community such an amazing place to learn, inspire, and create.

## Getting Started

### Prerequisites

You will need the following tools installed on your system:

- **Rust**: We use the latest stable version.
- **protobuf-compiler**: Required for some dependencies.
- **pkg-config**, **libssl-dev**: Common build dependencies.

### Local Development Setup

1.  **Fork the repository** on GitHub.
2.  **Clone your fork** locally:
    ```bash
    git clone https://github.com/YOUR_USERNAME/mylm.git
    cd mylm
    ```
3.  **Build the project** to ensure everything is working:
    ```bash
    cargo build
    ```
4.  **Run the tests**:
    ```bash
    cargo test
    ```

## Development Workflow

1.  Create a new branch for your feature or fix:
    ```bash
    git checkout -b feature/amazing-feature
    ```
2.  Make your changes.
3.  Ensure your code is formatted correctly:
    ```bash
    cargo fmt
    ```
4.  Run clippy to catch common mistakes:
    ```bash
    cargo clippy
    ```
5.  Commit your changes with meaningful commit messages.

## Pull Request Guidelines

-   **Descriptive Title**: clear summary of what the PR does.
-   **Description**: Detailed explanation of changes, reasoning, and any testing done.
-   **Link Issues**: If it fixes an open issue, reference it (e.g., `Fixes #123`).
-   **Small & Focused**: Keep PRs focused on a single issue or feature to make review easier.

## Code Style

-   Follow standard Rust idioms.
-   Use `cargo fmt` to format your code.
-   Add comments for complex logic.
-   If you add a new feature, please add a corresponding test case if possible.

## Reporting Issues

If you find a bug or have a feature request, please search the issue tracker first to see if it has already been reported. If not, open a new issue with as much detail as possible, including:

-   Steps to reproduce.
-   Expected vs. actual behavior.
-   Environment details (OS, terminal, version).

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
