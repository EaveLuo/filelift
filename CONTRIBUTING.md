# Contributing

Thanks for helping improve `filelift`.

## Pull Request Standard

Every pull request should describe the change, explain why it is needed, and
include appropriate tests.

Code changes should include:

- Unit tests for changed module logic.
- Integration tests for changed CLI behavior.
- A passing local verification run.

Run these before opening a pull request:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

## CI

The GitHub Actions CI workflow runs formatting, clippy, and the full test suite
on pull requests and pushes to `main`.

To make CI mandatory before merging, enable branch protection or repository
rulesets for `main` and require the `Rust checks` status check.
