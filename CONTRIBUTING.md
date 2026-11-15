# Contributing to Zara

Thanks for helping improve Zara.

## Development setup

1. Fork and clone the repository.
2. Install [Node.js](https://nodejs.org/) 20+, [Rust](https://www.rust-lang.org/tools/install), and [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).
3. Run `npm install`.
4. Copy `.env.example` to `.env` and add your own API keys.
5. Start the app with `npm run tauri dev`.

Never commit `.env` or real secrets.

## Pull requests

- Keep changes focused; prefer one concern per PR.
- Match the existing architecture: system/audio/API work in Rust, UI in React.
- Prefer clear naming over comments; add a comment only when intent is non-obvious.
- Remove dead code, unused imports, and leftover template assets when you replace them.
- Update docs when behavior or setup steps change.

## Commit style

Use short, imperative messages that explain why the change exists, for example:

- `feat: add streaming tts cancellation`
- `fix: recover mic session after Gemini disconnect`
- `docs: clarify .env setup in README`

## Questions

Open an issue if something in the docs or setup is unclear.
