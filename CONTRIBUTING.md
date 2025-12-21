# Contributing to Aperture

Thank you for your interest in contributing to Aperture! This document provides guidelines and information for contributors.

## Getting Started

1. **Fork the repository** on GitHub
2. **Clone your fork** locally:
   ```bash
   git clone https://github.com/YOUR_USERNAME/aperture.git
   cd aperture
   ```
3. **Create a new branch** for your feature or bugfix:
   ```bash
   git checkout -b feature/your-feature-name
   ```

## Development Setup

### Prerequisites

- Rust 1.70.0 or later
- Git

### Local Development

```bash
# Install dependencies and build
cargo build

# Run tests
cargo test

# Run formatting and linting
cargo fmt
cargo clippy -- -D warnings

# Run integration tests
cargo test --test '*'
```

### Pre-commit Hooks

This project uses pre-commit hooks to ensure code quality. The hooks will run automatically on commit and check:

- Code formatting (`cargo fmt --check`)
- Linting (`cargo clippy -- -D warnings`)
- Tests (`cargo test`)

## Code Guidelines

### Rust Code Style

- Follow the official [Rust Style Guide](https://doc.rust-lang.org/nightly/style-guide/)
- Use `cargo fmt` to format your code
- Address all `cargo clippy` warnings
- Write comprehensive tests for new functionality

### Testing

- **Unit tests**: Place in the same file as the code being tested
- **Integration tests**: Place in the `tests/` directory
- **Test coverage**: Aim for high test coverage of new code
- **Test isolation**: Ensure tests can run in parallel safely

### Documentation

- Document all public APIs with rustdoc comments
- Update README.md for user-facing changes
- Update relevant docs in `docs/` (guide.md, security.md, configuration.md, agent-integration.md)
- Create ADRs (Architecture Decision Records) for significant design decisions
- Update CHANGELOG.md following [Keep a Changelog](https://keepachangelog.com/) format

## Commit Guidelines

Follow [Conventional Commits](https://www.conventionalcommits.org/) specification:

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

### Types

- `feat`: A new feature
- `fix`: A bug fix
- `docs`: Documentation only changes
- `style`: Changes that do not affect the meaning of the code
- `refactor`: A code change that neither fixes a bug nor adds a feature
- `perf`: A code change that improves performance
- `test`: Adding missing tests or correcting existing tests
- `chore`: Changes to the build process or auxiliary tools

### Examples

```
feat(config): add base URL management commands
fix(executor): handle timeout errors properly
docs: update installation instructions
test(engine): add integration tests for command generation
```

## Pull Request Process

1. **Ensure tests pass**: Run `cargo test` before submitting
2. **Update documentation**: Include relevant documentation updates
3. **Add tests**: Include tests that cover your changes
4. **Update CHANGELOG**: Add an entry describing your changes
5. **Small, focused PRs**: Keep pull requests focused on a single feature or fix
6. **Descriptive title**: Use a clear, descriptive title for your PR

### PR Template

When creating a pull request, please include:

- **Description**: Clear description of what the PR does
- **Motivation**: Why is this change needed?
- **Testing**: How has this been tested?
- **Breaking changes**: Any breaking changes and migration notes
- **Related issues**: Link to any related issues

## Code Review Process

- All submissions require review before merging
- Reviewers will check for code quality, test coverage, and adherence to guidelines
- Address feedback promptly and professionally
- Be open to suggestions and improvements

## Release Process

Releases are managed using `cargo-release` and automated via GitHub Actions:

1. Maintainers run `./scripts/release.sh`
2. This creates a git tag and pushes it
3. GitHub Actions builds binaries for all platforms
4. Release is automatically published with binaries attached

## Getting Help

- **Documentation**: Check the README and inline documentation
- **Issues**: Search existing issues before creating new ones
- **Discussions**: Use GitHub Discussions for questions and ideas
- **Code Review**: Don't hesitate to ask questions during code review

## Project Structure

```
aperture/
├── src/                    # Main source code
│   ├── config/            # Configuration management
│   ├── engine/            # Dynamic CLI generation and execution
│   ├── cache/             # Spec caching and models
│   └── ...
├── tests/                 # Integration tests
├── docs/                  # Project documentation
│   └── adr/              # Architecture Decision Records
├── scripts/               # Development and release scripts
└── .github/              # GitHub workflows and templates
```

## Code of Conduct

This project follows the [Rust Code of Conduct](https://www.rust-lang.org/policies/code-of-conduct). Please be respectful and inclusive in all interactions.

## License

By contributing to Aperture, you agree that your contributions will be licensed under the MIT License.

---

Thank you for contributing to Aperture! 🚀