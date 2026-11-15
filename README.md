<p align="center">
  <img src="public/logo.svg" alt="Zara logo" width="128" height="128" />
</p>

<h1 align="center">Zara</h1>

<p align="center">
  A Bangla-first desktop voice assistant built with Tauri, React, Groq, and Gemini.
</p>

<p align="center">
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/License-MIT-7db892?style=flat-square" /></a>
  <img alt="Tauri 2" src="https://img.shields.io/badge/Tauri-2-24C8DB?style=flat-square&logo=tauri&logoColor=white" />
  <img alt="React 19" src="https://img.shields.io/badge/React-19-61DAFB?style=flat-square&logo=react&logoColor=black" />
  <img alt="Rust" src="https://img.shields.io/badge/Rust-edition%202024-DEA584?style=flat-square&logo=rust&logoColor=black" />
</p>

---

Zara listens, thinks, and speaks in natural Bangla (with light Bangla–English code-switching). Speech-to-text and text-to-speech run through Google Gemini Live; replies come from Groq. API keys stay in `.env` on the Rust backend and are never sent to the frontend.

## Features

- Hands-free voice loop: listen → think → speak
- Bangla-first conversational replies
- Streaming TTS with interrupt/cancel support
- Live mic capture and local audio playback
- Missing-key guidance when `.env` is incomplete

## Stack

| Layer | Tech |
| --- | --- |
| Desktop shell | [Tauri 2](https://tauri.app/) |
| UI | React 19 + TypeScript + Vite |
| Backend | Rust |
| LLM | Groq (`openai/gpt-oss-120b`) |
| STT / TTS | Google Gemini Live |

## Prerequisites

- [Node.js](https://nodejs.org/) 20+
- [Rust](https://www.rust-lang.org/tools/install) (stable)
- Platform deps for [Tauri 2](https://v2.tauri.app/start/prerequisites/)
- A [Groq API key](https://console.groq.com/keys)
- A [Google AI Studio API key](https://aistudio.google.com/apikey)

## Setup

```bash
git clone https://github.com/ars2k03/zara.git
cd zara
npm install
cp .env.example .env
```

Edit `.env`:

```env
GROQ_API_KEY=your_groq_key
GOOGLE_API_KEY=your_google_key
```

Keys are loaded only by the Rust backend. Do not commit `.env`.

## Development

```bash
npm run tauri dev
```

Frontend-only Vite preview (no native voice pipeline):

```bash
npm run dev
```

## Build

```bash
npm run tauri build
```

Installers land under `src-tauri/target/release/bundle/`.

## Project layout

```text
src/                 React UI (voice screen, hooks)
src-tauri/src/       Rust backend (audio, Gemini, Groq, commands)
public/logo.svg      Brand mark
.env.example         Required environment variables
```

## Security

- Secrets live only in `.env` (gitignored)
- Frontend never receives raw API key values
- See [SECURITY.md](SECURITY.md) to report vulnerabilities

## Contributing

Issues and pull requests are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## License

[MIT](LICENSE) © 2026 ars2k03
