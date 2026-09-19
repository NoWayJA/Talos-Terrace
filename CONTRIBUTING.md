# Contributing to Talos Terrace

Thank you for considering it. Talos Terrace is early and the direction is set, so the most useful
contributions right now are bug reports against real clusters, fixes, and small, well-tested
features that fit the roadmap. Open an issue before starting anything large.

## Before you start

- Read `README.md` for what Terrace is and `CLAUDE.md` for how the code is organised and the
  rules it follows. Those rules apply to every contributor, not only to AI assistants; the
  ones about who commits describe the maintainer's own setup and do not stop you committing
  to your fork.
- **General, not one cluster.** Nothing in the code may be specific to one cluster, one
  vendor or one project: facts come from the Talos and Kubernetes APIs, links go to upstream
  documentation, integrations are optional.
- **Never share credentials.** Redact kubeconfigs, talosconfigs, tokens and keys from issues,
  pull requests and logs. Addresses and hostnames are yours to redact as you see fit.

## Developing

```bash
npm ci                 # exact versions from package-lock.json
npm run dev            # the app, with hot reload (`web:dev` is the Vite server alone)
npm run check          # prettier, eslint, tsc, vitest
npm run rust:check     # cargo fmt, clippy -D warnings, cargo test
```

Both `check` scripts must pass before you open a pull request; CI runs exactly them on Linux,
macOS and Windows. The platform dependencies Tauri needs are listed in the README.

`npm run links:check` verifies the documentation links the canvas offers (network; CI runs it
weekly). If you add a link to `src/pages/Canvas/docs.ts`, run it once.

To see what Terrace reads from *your* cluster without launching the app:

```bash
cd src-tauri
cargo run --example probe -- --help
```

## Conventions that reviewers will hold you to

- Tests come with the code. Pure logic (fact builders, parsers, settings, vault, database) has
  unit tests; React Flow is not rendered in tests, the store is tested instead.
- One error type on the Rust side (`TerraceError`); every command is `async`, snake_case, and
  takes camelCase JSON. A new command is added to `lib.rs` and to `services/api.ts` in the
  same change. Components never call `invoke`.
- CSS uses the design tokens; no inline styles; class names are BEM-lite.
- Dependencies are pinned exactly; bump them on purpose, in their own commit.
- Files lifted from Modz keep their attribution header.

## Commits and pull requests

- Conventional commits: `feat:`, `fix:`, `chore:`, `docs:`, `test:`, `refactor:`.
- One topic per pull request, with a description of what changed and how you tested it
  (which Talos and Kubernetes versions, how many nodes).
- By contributing you agree that your contribution is licensed under the Apache License 2.0,
  the same as the project. There is no contributor licence agreement.

## Security

Do not report vulnerabilities in issues. See `SECURITY.md`.
