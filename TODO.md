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

---

### What is already true in the code (verified, do not re-implement)

- Browse-all mode already exists end to end: `ConnectionDialog.browse_all_databases`
  (`src/app/state.rs`), `ConnectionProfile.browse_all_databases()` / `maintenance_database()`
  (`src/connections/profile.rs`), `db::list_databases` (`src/db/introspect.rs`),
  `start_load_databases` / `start_post_connect_load` (`state.rs`), and `render_databases`
  (`src/ui/sidebar/mod.rs`).
- It is just OFF by default and hidden behind a checkbox, so a new connection only fetches one DB.
- `list_databases` already filters templates and DBs without CONNECT privilege. Keep that.


## TASK 1 — New connections fetch ALL databases on the server by default

### 1.1 Make browse-all the default
File: `src/app/state.rs`
- `impl Default for ConnectionDialog` (~line 130): `browse_all_databases: true`
- `ConnectionDialog::open_add` (~line 152): `self.browse_all_databases = true;`
- Leave `open_edit` as is (it reads the mode from the stored profile, so old single-DB profiles stay single-DB).

### 1.2 Add an optional "initial database" to the profile (backward compatible)
Why: Postgres needs *some* database for the first connection. Today it is hard-coded to `postgres`, which
fails on hosted servers where that DB is missing or not allowed (e.g. `defaultdb`, `neondb`).

File: `src/connections/profile.rs`
- Add to `ConnectionProfile`:
  ```rust
  /// Only used in browse-all mode: database for the first server connection.
  #[serde(default)]
  pub initial_database: Option<String>,
  ```
- Change `maintenance_database()`:
  ```rust
  pub fn maintenance_database(&self) -> &str {
      if let Some(db) = self.database.as_deref().filter(|d| !d.is_empty()) {
          return db;
      }
      if let Some(db) = self.initial_database.as_deref().filter(|d| !d.is_empty()) {
          return db;
      }
      "postgres"
  }
  ```
- Run `grep -rn "ConnectionProfile {" src tests` and add `initial_database: None` to every struct literal
  that fails to compile (at minimum `ConnectionDialog::build_profile`; also any test fixtures).

File: `src/app/state.rs`
- `build_profile` (~line 187): trim inputs and split by mode:
  ```rust
  let db_text = self.database.trim().to_string();
  let (database, initial_database) = if self.browse_all_databases {
      (None, if db_text.is_empty() { None } else { Some(db_text) })
  } else {
      (Some(db_text), None)
  };
  ```
  and pass both fields into the struct.
- `open_edit` (~line 169): replace the `self.database = ...` line with
  ```rust
  self.database = if profile.browse_all_databases() {
      profile.initial_database.clone().unwrap_or_default()
  } else {
      profile.database.clone().unwrap_or_default()
  };
  ```

### 1.3 Validate single-DB mode (prevents the silent "empty db = browse all" surprise)
File: `src/app/state.rs`, `ConnectionDialog::start_test` (~line 216), at the very top:
```rust
if !self.browse_all_databases && self.database.trim().is_empty() {
    self.error = Some("Enter a database name, or tick \"Show all databases\".".into());
    return;
}
```

### 1.4 Robust first connection with fallback (no more "database postgres does not exist")
File: `src/db/connection.rs` — add (and `use crate::connections::ConnectionProfile;`):
```rust
/// Connect to the server. In browse-all mode, if the first database is missing or
/// not allowed, try `postgres` then `template1`. Auth/network errors are NOT retried.
pub async fn connect_server(profile: &ConnectionProfile, password: &str) -> Result<PgPool> {
    let mut candidates = vec![profile.maintenance_database().to_string()];
    if profile.browse_all_databases() {
        for fb in ["postgres", "template1"] {
            if !candidates.iter().any(|c| c == fb) {
                candidates.push(fb.to_string());
            }
        }
    }
    let mut first_err: Option<anyhow::Error> = None;
    for name in candidates {
        let url = profile.connection_url_for_database(password, &name);
        match connect_with_url(&url).await {
            Ok(pool) => return Ok(pool),
            Err(e) => {
                let msg = e.to_string();
                let retryable = msg.contains("does not exist") || msg.contains("permission denied");
                if first_err.is_none() {
                    first_err = Some(e);
                }
                if !retryable {
                    break;
                }
            }
        }
    }
    Err(first_err.expect("at least one candidate"))
}
```
File: `src/db/mod.rs` — export it: `pub use connection::{connect, connect_server, connect_with_url};`

Replace the three places that connect to the server with it:
1. `ConnectionDialog::start_test` → build `let profile = self.build_profile(); let password = self.password.clone();`
   then `Promise::spawn_async(async move { crate::db::connect_server(&profile, &password).await })`.
2. `AppState::start_connect` (~line 962) → same pattern with the `profile` and `password` it already has
   (clone before moving into the async block; keep the `self.query.status` / `active_id` lines unchanged).
3. `AppState::submit_password_prompt` (~line 1058) → same, using `prompt.password.clone()`.
After this, if `ConnectionDialog::connection_url` is unused, delete it (clippy dead-code).
Do NOT change `connection_url_for_database` callers used for per-database pools.

### 1.5 Fix duplicate detection for the new modes
File: `src/app/state.rs`, `find_duplicate` (~line 976): compare databases normalized, so `None` and `Some("")` match:
```rust
&& p.database.as_deref().unwrap_or("") == profile.database.as_deref().unwrap_or("")
```

### 1.6 Percent-encode credentials in URLs (passwords with `@ : / # ? %` currently break the URL)
File: `src/connections/profile.rs` — add a private helper (no new crate):
```rust
fn pct(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{:02X}", b),
        })
        .collect()
}
```
Use `pct(&self.username)`, `pct(password)`, and `pct(database)` in BOTH `connection_url` and
`connection_url_for_database`. Add unit tests: password `p@ss:w/rd#1?` round-trips into a URL
without raw `@ / # ?` in the credentials part.

### 1.7 Labels
- `src/ui/connection_dialog.rs`: window title "Add Database"/"Edit Database" → "Add Connection"/"Edit Connection".
- Checkbox text → `"Show all databases on this server"`; tooltip → `"Lists every database you can connect to. The field below is only used for the first connection."`
- Replace the Database block: label becomes `"Initial database (optional)"` when browse-all is on, `"Database"` when off.
  The field must be ENABLED in both modes now (remove `add_enabled_ui(!dialog.browse_all_databases, ...)`),
  hint text `"postgres"`.
- `src/ui/sidebar/mod.rs` (~line 30): tooltip `"Add Database"` → `"Add Connection"`.

### Task 1 acceptance (manual)
- [ ] Click "+" → checkbox is already ticked; Database field is optional; Test & Connect with it empty succeeds.
- [ ] Sidebar immediately lists every non-template DB the user can CONNECT to; status bar says "Connected — N database(s)".
- [ ] Expanding a DB lazy-loads its schemas/tables (unchanged behaviour).
- [ ] Untick + empty Database → inline error, no connection attempt.
- [ ] Wrong password → ONE attempt only (no fallback retries), error shown.
- [ ] Server without a `postgres` DB but with "Initial database" filled → connects.
- [ ] Old saved single-DB profile (`"database": "mydb"`) still opens as single-DB; `connections.json` without `initial_database` still loads.
- [ ] Password containing `@`, `:`, `/`, `#` connects.

---

## TASK 2 — Connection dialog looks dark / opaque / stuck

Three separate causes. Fix all three.

### 2A (REAL BUG, do first) Dialog never closes after a successful connect and the success branch re-runs every frame
File: `src/app/state.rs`, block `// Poll dialog test promise` (~line 1220).
- In the `Some(Ok(pool))` branch `test_promise` is never cleared (only the error branches clear it), and
  `upsert_profile_and_connect` never closes the dialog. Result: the dim backdrop + dialog stay on screen, and every
  frame it re-saves the secret, rewrites `connections.json`, and calls `finish_connect` → `clear_query_state`
  (reloads the tree, causes flicker/"darkened" screen). The duplicate branch has the same loop (warning reappears after Cancel).
- Fix: right after `let pool = pool.clone();` add `self.connection.dialog.test_promise = None;`
  (the borrow of `p` ends after the clone, so this compiles). Then:
  - success, no duplicate: after `self.upsert_profile_and_connect(profile, password, pool);` call `self.connection.dialog.close();`
  - duplicate branch: keep dialog open (it needs the "Save anyway" button) — promise is already `None` now.
  - keyring error branch: keep the existing lines (already clears the promise).
- Do NOT close the dialog if `upsert_profile_and_connect` failed to save the profile. Make it return `bool`
  (`false` on the `repo.save` error path, which already sets `query.status`), and only close on `true`.
  Update its other callers (`commit_pending_save`, `confirm_duplicate_save`, password-prompt path) for the new return type.

### 2B Backdrop can paint ON TOP of the dialog (dialog looks dimmed)
Backdrop `Area` and `Window` are both `Order::Middle`; z-order depends on creation order and any `move_to_top`.
File: `src/ui/connection_dialog.rs`
- Keep backdrop at `Order::Middle`; add `.order(egui::Order::Foreground)` to the `egui::Window` builder.
- Apply the same one-line change to the windows in `src/ui/password_prompt.rs`,
  `src/ui/results/edit_dialog.rs`, `src/ui/results/row_edit_dialog.rs` (same pattern, same latent bug).
  Do not change the backdrop `Area` order in any of them.

### 2C Backdrop does not block clicks (user can click the sidebar/"+" behind the dialog, which resets the form)
File: `src/ui/connection_dialog.rs`, inside the backdrop `Area` closure, after `rect_filled`:
```rust
ui.allocate_rect(screen, egui::Sense::click_and_drag());
```
This MUST ship together with 2B (a clickable backdrop moves itself to top; 2B keeps the window above it).
Apply to the other three dialogs only if they show the same behaviour when tested.

### 2D Visual: dialog is `#18181b` on a near-black dim, so it reads as "dark/opaque"
File: `src/ui/connection_dialog.rs`
- Window frame: `.fill(theme.surface)` instead of `theme.panel`; keep the 1px `theme.border` stroke.
- Add a shadow so it lifts off the background:
  ```rust
  .shadow(egui::Shadow { offset: [0, 8], blur: 24, spread: 0, color: egui::Color32::from_black_alpha(120) })
  ```
- Backdrop: `egui::Color32::from_black_alpha(110)` instead of `from_rgba_premultiplied(0, 0, 0, 150)`.
- Check that text inputs still contrast with the new fill (they use `extreme_bg_color`); adjust only
  `connection_dialog.rs`, do not change `theme.rs` constants (used app-wide).
- If any `egui::Shadow` field type errors on 0.36, use the field types the compiler reports; do not change egui version
  (note: `Cargo.lock` contains egui 0.35 and 0.36.2 — the app uses 0.36.2).

### Task 2 acceptance (manual)
- [ ] Open "+": dialog is clearly lighter than the dimmed background, with border and shadow; all text readable.
- [ ] Click anywhere on the dim area: dialog stays on top and un-dimmed; nothing behind it reacts.
- [ ] Test & Connect success (new profile): dialog closes within one frame, sidebar shows the server, no flicker, CPU idle afterwards.
- [ ] Duplicate server: warning shows once; "Cancel" dismisses it and it does NOT come back; "Save anyway" saves and closes.
- [ ] Failure (bad password/host): error stays in dialog, dialog stays open, "Test & Connect" re-enabled.
- [ ] Edit an existing profile: pre-filled correctly (including "Initial database"), saving closes the dialog.
- [ ] Password prompt and row/cell edit dialogs still open, dim, and close normally.

---

## Do NOT touch
`src/db/introspect.rs`, `src/db/query.rs`, `src/query_builder.rs`, `src/services/secrets.rs`, results table code, theme constants,
`Cargo.toml`. If a change seems to require one of these, stop and add a note to this file instead.

---

## Implementation notes (agent)

- **Follow-up: the browse-all checkbox is GONE — the Database field alone decides the mode**
  (per the "small plan: empty Database field = show all databases").
  `ConnectionDialog.browse_all_databases` was removed (struct, `Default`, `open_add`,
  `open_edit`); `build_profile` now derives the mode from the trimmed field
  (`""`/whitespace => `database: None` => browse-all); dialog label is
  "Database (leave empty to show all)" with hint "all databases", checkbox deleted.
  Consequences worth knowing:
  - Task 1.3's validation ("Enter a database name, or tick Show all databases") is
    **removed** — an empty field is now the normal browse-all path, not an error.
  - Task 1.7's checkbox/tooltip text above is **obsolete**; the title/tooltip/sidebar
    relabels still apply.
  - `ConnectionProfile::initial_database` is now **read-only from the UI**: the field
    stays on the profile (old `connections.json` still loads and `maintenance_database()`
    still honours it) but `build_profile` always writes `None`, so re-saving an old
    browse-all profile drops its initial database. First-connection fallback
    (`connect_server`: maintenance db => `postgres` => `template1`) covers the rest.
- **Pre-existing `cargo clippy -- -D warnings` failure fixed outside the named task files.**
  `src/services/mod.rs` re-exported `KeyringStore`, which is never referenced outside
  `services/secrets.rs` itself, so `clippy -D warnings` failed on unmodified `HEAD`
  (verified by stashing). Removed it from the `pub use` line — `SecretStore` /
  `VaultStore` / `MockSecretStore` re-exports are unchanged. This was required to
  satisfy the "fix anything clippy reports" rule; `services/secrets.rs` was NOT touched.
- **2C applied only to `connection_dialog.rs`** as specified: the other three dialogs'
  backdrops still let clicks through to the UI behind them (same latent bug), but the
  instruction conditions that change on manual testing, which is not possible here.
  Their windows are now `Order::Foreground` (2B), so they are at least no longer
  painted *under* their own backdrop.
- **`ConnectionProfile::connection_url` kept** (used by unit tests / public API);
  only the now-unused `ConnectionDialog::connection_url` was deleted, as instructed.
- The pre-existing keyring bug at the top of this section
  ("Failed to get password: ... No matching entry found") was NOT addressed — it lives
  in `src/services/secrets.rs` / secret-store lookup, both on the do-not-touch list.
- **New: "Add new database" submenu on the connection context menu** (requested as
  "on the conn admin add an sub menu to add new db"). Scope decision: this was read as
  *create a database on the server* (the connection card's right-click menu, a.k.a.
  "conn admin", already had Connect/Edit/New schema/Delete). The other reading — open
  the Add Connection dialog — was skipped because the sidebar "+" already does that.
  Files: `src/ui/sidebar/mod.rs` (submenu), `src/app/state.rs`
  (`SchemaNameMode::CreateDatabase`, `SchemaNameDialog::template`,
  `AppState::open_create_database`, confirm/poll branches, `validate_database_name`),
  `src/ui/schema_name_dialog.rs` (title/action/hint/template line).
  - Submenu is on the connection card, after "Edit". Items: "Empty database…" and,
    when the active connection has a loaded database list, one "Clone from <db>" entry
    per database (`CREATE DATABASE new TEMPLATE src`). Disabled (greyed) when the
    connection is not active — `CREATE DATABASE` needs a live session.
  - The dialog is the existing `SchemaNameDialog` reused with a third mode; it runs
    against the active pool (`schema_pool(None)`), always in the maintenance DB, so it
    works in both browse-all and single-database modes.
  - SQL is built in `state.rs` with `crate::db::query::quote_ident` and executed via
    `sqlx::raw_sql(...).execute(...)`; `src/db/query.rs`/`introspect.rs` were NOT touched
    (they re-export nothing for this and are on the do-not-touch list). No crates added.
  - Database names are validated inline (`validate_database_name`: non-empty, <=63 bytes)
    rather than with `validate_schema_name`, which rejects `pg_*` and has schema wording.
  - On success: status `Database "<name>" created`; in browse-all mode the database list
    is reloaded (`start_load_databases`). In single-database mode nothing needs a refresh.
  - Clone sources come from `query.databases`, which `clear_query_state` empties on every
    connection switch, so they can never be stale from another connection.
  - Gates still green after this change: `cargo build`, `cargo clippy -- -D warnings`
    (the TODO-mandated form), `cargo test` = 30. Note `cargo clippy --all-targets`
    additionally fails on two **pre-existing** items outside this change (unused
    `MockSecretStore` test re-export in `services/mod.rs`, unused helper in
    `tests/autocomplete_focus.rs`); the named gate is the non-`--all-targets` form.
  - NOT verified interactively (no live Postgres / GUI here): the submenu hover/arrow
    rendering and the click-closes-the-whole-menu behaviour of nested `menu_button`s.
