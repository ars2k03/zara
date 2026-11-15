# Security Policy

## Supported versions

Security fixes are applied to the latest commit on `main`.

## Reporting a vulnerability

Please **do not** open a public issue for security problems.

Email **arafatsohan2003@gmail.com** with:

- A short description of the issue
- Steps to reproduce or a proof of concept
- Impact assessment if you have one

You should receive an acknowledgment within a few days. Please give a reasonable window for a fix before any public disclosure.

## Secrets

- Store API keys only in `.env` (gitignored)
- Use `.env.example` as the template for required variable names
- The Rust backend reads secrets; key values must never be sent to the frontend
