# pgTool — TODO & Security Notes

## Security Improvements Needed

### Connection storage
- [ ] `connections.json` stores profiles in plaintext — consider encrypting the file at rest
  or using Windows DPAPI / platform credential store for the entire profile, not just the
  password.
- [ ] Password is stored via `keyring` (Windows Credential Manager) which is good, but
  keyring access is not gated — any process running as the same user can read credentials.
  Consider adding user-level access control or a master password.
- [ ] The connection ID is derived from `postgres://{host}:{port}/{database}?sslmode={mode}`.
  This is deterministic and unique per server config, but it means two profiles pointing at
  the same server will collide. This is intentional (deduplication), but the user should be
  warned if they try to save a duplicate.

### Keyring fallback
- [ ] If `keyring` fails (e.g. no secret service on headless Linux), the app currently
  silently ignores the error. Add a fallback: prompt for password each session, or store
  encrypted in a local file with a warning.

### Connection URL logging
- [ ] `db::connect_with_url` logs the connection attempt. Make sure the password is never
  included in log output (currently it is part of the URL string — consider redacting it
  before logging).

### In-memory password handling
- [ ] Passwords are held as `String` in `ConnectionDialog` and passed around as owned
  strings. Consider using `SecretString` from the `secrecy` crate to prevent accidental
  logging or display of credentials.

## Architecture TODOs

### Phase 4 follow-ups
- [ ] Connection edit flow: when editing, pre-fill the dialog from the stored profile.
  Currently `open_edit` works but the test-promise handler needs to update the existing
  profile instead of creating a duplicate (handled via upsert now).
- [ ] Remember last active connection across sessions (save `active_id` to config).

### Phase 5 follow-ups
- [ ] Results table: add column type detection for right-alignment of numeric columns.
- [ ] Results table: keyboard navigation (arrow keys to move between cells).

### Phase 6 follow-ups
- [ ] Scrollbar thumb color should be `accent_hover` — egui 0.29 doesn't expose a direct
  API for this, it uses `widgets.inactive` / `widgets.hovered` fills. May need a custom
  paint pass or wait for egui to expose scrollbar styling.
- [ ] Focused text input border: egui uses `extreme_bg_color` + widget stroke for focus
  indication. Set `widgets.active.fg_stroke` to accent color for focus ring effect.




[TASKS AND BUGS TO RESOLVE NOW]

WHEN I TRY TO OPEN AN EXITING DB CONNECTION SAVED already BEFORE IN ANOTHER SESSION IT SAYS: Failed to get password: keyring get_password failed: No matching entry found in secure storage
