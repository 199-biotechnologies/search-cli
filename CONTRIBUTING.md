# Contributing to Search CLI

Thanks for your interest in contributing. Here's how to get started.

## Setup

```bash
git clone https://github.com/199-biotechnologies/search-cli
cd search-cli
cargo build
cargo test
```

Requires Rust 1.75+ and Cargo.

## Making Changes

1. Fork the repo and create a branch from `master`
2. Make your changes
3. Run `cargo test` and `cargo clippy` to check for issues
4. Open a pull request with a clear description of what you changed and why

## What We're Looking For

- New search provider integrations
- Bug fixes and performance improvements
- Better error messages and documentation
- Test coverage improvements

## Code Style

- Follow existing patterns in the codebase
- Keep functions focused and small
- Add tests for new functionality
- Use meaningful variable names

## Reporting Issues

Open a GitHub issue with:
- What you expected to happen
- What actually happened
- Steps to reproduce
- Your OS and Rust version

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
