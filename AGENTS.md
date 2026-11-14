# AGENTS.md

Rules for all code generation and maintenance in this repository.

## Code Style

- Code must be fully clean and architected: every file has one clear responsibility, and logic lives in the correct layer (system access in the Rust backend, UI in React).
- No unnecessary comments. Code must be self-documenting through naming and structure; a comment is allowed only when it explains a non-obvious necessity.
- Never leave old, dummy, or template code behind. Dead code, unused imports, unused assets, and placeholder leftovers must be removed as soon as they are replaced.
- Always write the latest stable idioms of each language/framework (current Rust edition, current React, current TypeScript) — never outdated patterns.
- Match the existing architecture and naming conventions; do not introduce one-off patterns.

## Secrets

- Secrets live only in `.env` (gitignored) and are read exclusively by the Rust backend; key values are never sent to the frontend.
