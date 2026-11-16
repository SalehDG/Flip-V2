# Flip V2 — Phase 2 Gmail Sync implementation notes

## Implemented in this checkpoint

- Read refresh token from Windows Credential Manager.
- Refresh OAuth access token using the existing OAuth client configuration.
- Gmail API client for profile, message list, message detail and attachment retrieval.
- Initial sync query based on the confirmed sender `no-reply@flip.id` and user-selected start date.
- Gmail pagination with persisted next-page token after each completed page.
- Resume behavior: an interrupted page can be replayed; `(gmail_account_id, gmail_message_id)` uniqueness prevents duplicate message rows.
- Gmail Message is stored independently from its Gmail Thread.
- Message MIME JSON and attachment metadata are persisted locally.
- Attachment bytes are checked using magic bytes; PNG and JPEG are accepted and original bytes are retained.
- Attachment paths are isolated by account and message.
- Tauri commands and a basic React initial-sync control with progress events.
- SQLite migration v2; migration v1 remains unchanged.

## Important scope boundary

This is an initial-sync implementation checkpoint, **not the complete Phase 2 acceptance package yet**.

Not implemented in this checkpoint:

- Gmail History API incremental synchronization and expired-history fallback.
- Persisted retry/backoff schedule across application restarts.
- Cancellation of an in-progress sync.
- Comprehensive mock-server integration tests for Gmail HTTP responses.
- A polished production Gmail Sync screen.

Automatic one-minute polling, system tray and background operation remain out of scope here, as required by the phase boundary in the continuation brief and SRS Phase 7.

## OAuth configuration

The existing implementation expects:

`secrets/oauth-client.json`

relative to the project root, because the Rust code resolves it from `src-tauri/../secrets/oauth-client.json`. Keep this file out of Git and out of shared ZIP uploads.

## Database migration

Migration v2 adds:

- `gmail_sync_state`
- `gmail_messages`
- `gmail_attachments`

The existing v1 schema is not rewritten. `transactions` continues to receive a `Detected` row keyed by account + Gmail Message ID, while raw message and attachment records are stored separately.

## Commands to validate on Windows

Run from project root in PowerShell:

```powershell
npm ci
npm run build
cargo test --manifest-path .\src-tauri\Cargo.toml
cargo check --manifest-path .\src-tauri\Cargo.toml
npm run tauri dev
```

The environment used to prepare this checkpoint did not have the Rust toolchain available, and the frontend dependency installation did not complete. Therefore no successful Cargo or frontend build is claimed here. Run the commands above before committing.
