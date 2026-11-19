# Notes for agents

- Read `docs/README.md` first; it links to the architecture, security model and decisions.
- Keep `docs/` true. If your change affects how something works, update the doc in the same commit. Plain English, short, only what's worth keeping.
- Plans, bugs and ideas go in GitHub issues (`gh issue list`), not in docs.
- Production-grade code: tests for every behavior change, no shortcuts left undocumented.
- Run the checks in `docs/development.md` before committing.
- Changes to crypto, membership or sync need an attack test in `crates/shy-core/src/workspace_tests.rs` and an update to `docs/security.md`.
- `.local/` is gitignored scratch space for notes, logs and throwaway files.
