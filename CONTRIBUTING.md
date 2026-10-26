# Contributing

Thanks for your interest in dropdl! Bug reports, ideas and pull requests are welcome.

## Before you start

dropdl is meant to stay small: one window, few controls, few dependencies, and good defaults instead of settings. For anything bigger than a fix, please open an issue first so we can agree on the approach before you write the code.

## Workflow

- `master` always holds the latest reviewed code. Nobody pushes to it directly.
- Work on a branch (`fix/…`, `feat/…`, `docs/…`) and open a pull request to `master`. The maintainer reviews and merges every pull request.
- CI runs `cargo test` on Windows and macOS for each pull request; it must pass.
- Keep a pull request to one topic, with commit messages that say what changed and why.

## Development

Requires Rust (stable, edition 2024). See the "For developers" section of the README for build and test commands, and [CLAUDE.md](CLAUDE.md) for the conventions the code follows (threads and messages instead of async, process handling, platform-specific code). The docs in `docs/` explain the architecture and the reasoning behind the download rules.

In short:

- Run `cargo test` before opening a pull request. Changes to downloads should also pass the end-to-end tests (`cargo test -- --ignored --nocapture`, see [docs/testing.md](docs/testing.md)).
- Update the docs in the same pull request when behavior changes, and add a line to `CHANGELOG.md` under the next version.
- New dependencies: check their size impact and license, and regenerate `THIRD-PARTY-LICENSES.txt` ([docs/release.md](docs/release.md)).
- Never put personal information (user names, home paths, emails) in code, docs or test fixtures. Recorded API responses must not come from a logged-in session.
- UI text is in English.

## License

By contributing, you agree that your contributions are dual licensed under MIT OR Apache-2.0, like the rest of dropdl (see the README).
