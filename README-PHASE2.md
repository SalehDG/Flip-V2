# Flip V2 — Phase 2 Gmail Synchronization

## Included in this update

- Initial Gmail synchronization with date filter and pagination.
- Per-account resumable checkpoints.
- Gmail History API incremental synchronization.
- Fallback to date-bounded initial synchronization when Gmail History ID expires.
- Message-level idempotency using `(gmail_account_id, gmail_message_id)`.
- Per-account in-process synchronization lock.
- Bounded exponential retry for network errors, HTTP 429, quota-related 403, and HTTP 5xx.
- One access-token refresh retry after Gmail returns HTTP 401.
- Persisted failure/authentication-required state and error message.
- PNG/JPEG attachment detection by magic bytes only.
- Attachment metadata includes detected format, size, SHA-256, and local path.
- Frontend controls for initial and manual incremental synchronization.
- SQLite migration version 3 for incremental sync checkpoints.

## OAuth configuration

The application resolves `oauth-client.json` in this order:

1. Tauri application config directory.
2. Bundled resource directory (`secrets/oauth-client.json` or root resource directory).
3. Development fallback at `../secrets/oauth-client.json` relative to `src-tauri`.

For a packaged Windows build, place the Google Desktop OAuth client JSON at the application config path shown in the error message if the file is not found. Rename the file to `oauth-client.json`.

Do not commit the OAuth JSON or refresh tokens to Git.

## Commands

From the project root:

```powershell
npm ci
npm run build
npm run tauri dev
```

Rust tests:

```powershell
cd src-tauri
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features
```

## Phase 2 behavior

- Initial sync is started manually by the user and uses the selected date plus `from:no-reply@flip.id`.
- Incremental sync is started manually and uses Gmail History API `messageAdded` records.
- The History API path filters messages to the same Flip sender before saving them.
- Expired History IDs trigger a full scan from the configured start date. Existing messages are skipped by message ID.
- Polling every minute, system tray, Windows startup, and background operation remain in the Background & Retention phase, as previously scoped.
- This phase stores each Gmail Message and its accepted PNG/JPEG attachments. Parsing transaction fields and OCR remain in later phases.

## Verification boundary

The SQLite migration SQL was exercised against SQLite and the new schema/status was verified. The TypeScript and Rust builds could not be run in this environment because the npm dependencies were not fully available and the Rust toolchain is not installed. Live Gmail API integration also requires the user's OAuth client and connected Gmail account.
