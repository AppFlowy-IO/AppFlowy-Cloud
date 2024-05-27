# Contributing <!-- omit in toc -->

First of all, thank you for contributing to AppFlowy Cloud! The goal of this document is to provide everything you need
to know in order to contribute to AppFlowy Cloud and its different integrations.

- [Assumptions](#assumptions)
- [How to Contribute](#how-to-contribute)
- [Development Workflow](#development-workflow)

## Assumptions

1. **You're familiar with [GitHub](https://github.com) and
   the [Pull Request](https://help.github.com/en/github/collaborating-with-issues-and-pull-requests/about-pull-requests)(
   PR) workflow.**
2. **You know about the [AppFlowy community](https://discord.gg/9Q2xaN37tV"). Please use this for help.**

## How to Contribute

Contributions are welcome! Here's how you can help improve AppFlowy Cloud:

1. Identify or propose enhancements or fixes by
   checking [existing issues](https://github.com/AppFlowy-IO/AppFlowy-Cloud/issues)
   or [creating a new one](https://github.com/AppFlowy-IO/AppFlowy-Cloud/issues/new/choose).
2. [Fork the repository](https://help.github.com/en/github/getting-started-with-github/fork-a-repo) to your own GitHub
   account. Feel free to discuss your contribution with a maintainer beforehand.
3. [Create a feature or bugfix branch](https://help.github.com/en/github/collaborating-with-issues-and-pull-requests/creating-and-deleting-branches-within-your-repository)
   in your forked repo.
4. Familiarize yourself with the [Development Workflow](#development-workflow) for guidelines on maintaining code
   quality.
5. Implement your changes on the new branch.
6. [Open a Pull Request (PR)](https://help.github.com/en/github/collaborating-with-issues-and-pull-requests/creating-a-pull-request-from-a-fork)
   against the `main` branch of the original AppFlowy Cloud repo. Await feedback or approval from the maintainers.

## Development Workflow

Before diving into development, familiarize yourself with the codebase and project standards by reviewing
the [Development Guide](./GUIDE.md).

### Setting Up the Local Server

To start the server on your local machine, run the following script:

```bash
./script/run_local_server.sh
```

### Testing

Verify that your changes work as expected by running the test suite:

```bash
cargo test
```

### Pull Request (PR) Requirements

For a pull request to be accepted, it must satisfy the following criteria:

1. **Pass All Tests**: Your PR should not break any existing functionality and must pass all the automated tests.

2. **Linting with Clippy**: Your code must adhere to the linting standards enforced
   by [`clippy`](https://github.com/rust-lang/rust-clippy). You can check for linting issues using:

   ```bash
   cargo clippy -- -D warnings
   ```

   If `clippy` isn't already installed:

   ```bash
   rustup update
   rustup component add clippy
   ```

3. **Code Formatting**: The code must comply with established formatting rules. Use the following commands for
   formatting and checking your code:

   To format your code:

   ```bash
   cargo fmt
   ```

   To validate formatting:

   ```bash
   cargo fmt --all -- --check
   ```
