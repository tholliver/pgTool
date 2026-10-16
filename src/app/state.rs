use crate::connections::{ConnectionProfile, SslMode};
use crate::db;
use crate::query_builder::QueryBuilder;
use crate::services::{ProfileRepo, SecretStore};
use poll_promise::Promise;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

#[derive(PartialEq)]
pub enum Focus {
    Schemas,
    Tables,
}

/// What kind of identifier an autocomplete candidate is; drives ranking
/// (tables first) and the popup's row icon.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CandidateKind {
    Schema,
    Table,
    Column,
}

/// One autocomplete suggestion: an identifier plus its kind.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Candidate {
    pub name: String,
    pub kind: CandidateKind,
}

/// Lazy connect+introspect promise for one expanded database:
/// resolves to (pool, schemas) for the keyed database name.
type DbSchemaPromise = Promise<anyhow::Result<(PgPool, Vec<db::SchemaInfo>)>>;

/// One in-flight lazy table-detail load: `(database, schema, table, promise)`.
/// `database` is `None` in single-database mode, and the promise resolves to
/// the table's indexes/foreign-keys/triggers.
type TableDetailPromise = Promise<anyhow::Result<db::TableDetails>>;

/// Identifier rules for a new database name (Postgres caps identifiers at 63
/// bytes; the name is quoted, so case and spaces are preserved).
fn validate_database_name(name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!name.is_empty(), "Database name cannot be empty");
    anyhow::ensure!(name.len() <= 63, "Database name is too long (max 63 bytes)");
    Ok(())
}

/// Modal password prompt for a saved connection with no stored secret.
#[derive(Default)]
pub struct PasswordPrompt {
    pub open: bool,
    pub profile_id: String,
    pub profile_name: String,
    pub password: String,
    pub remember: bool,
    pub error: Option<String>,
    pub submitting: bool,
    pub test_promise: Option<Promise<Result<PgPool, anyhow::Error>>>,
    /// If test succeeds and duplicate is found, hold the profile + pool here
    /// until user confirms.
    pub pending_duplicate: Option<(String, ConnectionProfile, PgPool)>,
}

impl PasswordPrompt {
    pub fn open(&mut self, profile: &ConnectionProfile) {
        self.open = true;
        self.profile_id = profile.id.clone();
        self.profile_name = profile.generated_name();
        self.password.clear();
        self.remember = true;
        self.error = None;
        self.submitting = false;
        self.test_promise = None;
        self.pending_duplicate = None;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.profile_id.clear();
        self.profile_name.clear();
        self.password.clear();
        self.error = None;
        self.submitting = false;
        self.test_promise = None;
        self.pending_duplicate = None;
    }
}

pub struct ConnectionState {
    pub profiles: Vec<ConnectionProfile>,
    pub active_id: Option<String>,
    pub active_pool: Option<PgPool>,
    /// Cached password for the active connection (used for browse-all database switching).
    pub active_password: Option<String>,
    pub dialog: ConnectionDialog,
    pub password_prompt: PasswordPrompt,
    pub repo: ProfileRepo,
    pub secrets: Arc<dyn SecretStore>,
    pub menu_open: bool,
}

pub struct ConnectionDialog {
    pub open: bool,
    pub editing_id: Option<String>,
    /// Stable UUID for this dialog session (generated on open_add, copied on open_edit).
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: String,
    /// Empty => browse-all mode: list every database on this server.
    pub database: String,
    pub username: String,
    pub password: String,
    pub ssl_mode: SslMode,
    pub error: Option<String>,
    pub testing: bool,
    pub test_promise: Option<Promise<Result<PgPool, anyhow::Error>>>,
    /// If test succeeds and duplicate is found, hold the warning + profile + pool
    /// until user confirms "Save anyway".
    pub pending_duplicate: Option<(String, ConnectionProfile, PgPool)>,
    /// Set by UI when user clicks "Save anyway" on duplicate warning.
    pub confirm_duplicate_save_requested: bool,
}

impl Default for ConnectionDialog {
    fn default() -> Self {
        Self {
            open: false,
            editing_id: None,
            id: String::new(),
            name: String::new(),
            host: "localhost".into(),
            port: "5432".into(),
            database: String::new(),
            username: "postgres".into(),
            password: String::new(),
            ssl_mode: SslMode::Disable,
            error: None,
            testing: false,
            test_promise: None,
            pending_duplicate: None,
            confirm_duplicate_save_requested: false,
        }
    }
}

impl ConnectionDialog {
    pub fn open_add(&mut self) {
        self.open = true;
        self.editing_id = None;
        self.id = Uuid::new_v4().to_string();
        self.name.clear();
        self.host = "localhost".into();
        self.port = "5432".into();
        self.database.clear();
        self.username = "postgres".into();
        self.password.clear();
        self.ssl_mode = SslMode::Disable;
        self.error = None;
        self.testing = false;
        self.test_promise = None;
    }

    pub fn open_edit(&mut self, profile: &ConnectionProfile, password: &str) {
        self.open = true;
        self.editing_id = Some(profile.id.clone());
        self.id = profile.id.clone();
        self.name = profile.name.clone();
        self.host = profile.host.clone();
        self.port = profile.port.to_string();
        self.database = profile.database.clone().unwrap_or_default();
        self.username = profile.username.clone();
        self.password = password.to_string();
        self.ssl_mode = profile.ssl_mode.clone();
        self.error = None;
        self.testing = false;
        self.test_promise = None;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.error = None;
        self.testing = false;
        self.test_promise = None;
        self.pending_duplicate = None;
        self.confirm_duplicate_save_requested = false;
    }

    pub fn build_profile(&self) -> ConnectionProfile {
        let port = self.port.parse::<u16>().unwrap_or(5432);
        // Empty Database field => browse-all mode (profile.database == None).
        let db = self.database.trim();
        let database = if db.is_empty() {
            None
        } else {
            Some(db.to_string())
        };
        let name = if self.name.trim().is_empty() {
            format!("{}:{}", self.host, port)
        } else {
            self.name.clone()
        };
        ConnectionProfile {
            id: self.id.clone(),
            name,
            host: self.host.clone(),
            port,
            database,
            username: self.username.clone(),
            ssl_mode: self.ssl_mode.clone(),
            ssl_ca_path: None,
            initial_database: None,
        }
    }

    pub fn start_test(&mut self) {
        self.error = None;
        self.testing = true;
        let profile = self.build_profile();
        let password = self.password.clone();
        self.test_promise = Some(Promise::spawn_async(async move {
            crate::db::connect_server(&profile, &password).await
        }));
    }
}

pub struct QueryState {
    /// Schemas of the single connected database (browse_all_databases == false).
    pub schemas: Vec<db::SchemaInfo>,
    /// Databases on the server (browse_all_databases == true); schemas lazy-loaded.
    pub databases: Vec<db::DatabaseInfo>,
    /// One pool per opened database in browse-all mode.
    pub database_pools: std::collections::HashMap<String, PgPool>,
    pub expanded_databases: std::collections::HashSet<String>,
    /// Database whose table was last selected — queries run against its pool.
    pub active_database: Option<String>,
    pub schema_idx: usize,
    pub table_idx: usize,
    pub focus: Focus,
    pub sql: String,
    pub result: Option<db::QueryResult>,
    pub status: String,
    pub schema_promise: Option<Promise<anyhow::Result<Vec<db::SchemaInfo>>>>,
    pub query_promise: Option<Promise<anyhow::Result<db::QueryResult>>>,
    /// Server-wide database list (browse-all mode).
    pub database_promise: Option<Promise<anyhow::Result<Vec<db::DatabaseInfo>>>>,
    /// Lazy connect+introspect for one expanded database.
    pub db_schema_promise: Option<(String, DbSchemaPromise)>,
    pub limit_value: String,
    pub offset_value: String,
    /// Current results page (0-indexed); pages always REPLACE the result set.
    pub current_page: u64,
    /// True when the last fetch returned a full page (probably more rows).
    pub has_next_page: bool,
    pub filter_text: String,
    /// Sidebar tree filter (matches table names, case-insensitive).
    pub tree_filter: String,
    /// SQL awaiting an explicit "run anyway" from the destructive-statement
    /// confirm dialog; None when no confirmation is pending.
    pub pending_confirm_sql: Option<String>,
    /// Abort handle for the running query task (cancellation).
    pub query_cancel: Option<tokio::task::AbortHandle>,
    /// Live autocomplete suggestions for the word under the caret (max 8).
    pub autocomplete_matches: Vec<Candidate>,
    /// Byte range in `sql` that a chosen suggestion replaces.
    pub autocomplete_splice: Option<std::ops::Range<usize>>,
    /// True when the caret word has live suggestions and the floating
    /// autocomplete popup is shown.
    pub autocomplete_open: bool,
    /// Highlighted row in the autocomplete popup.
    pub autocomplete_selected: usize,
    /// Caret position (in chars) to restore in the SQL editor after an
    /// autocomplete insert; consumed by the UI on the next editor draw.
    pub pending_caret: Option<usize>,
    /// The SQL editor's real egui id, captured from its TextEditOutput each
    /// frame (`id_salt` is hashed, so it cannot be fabricated elsewhere).
    pub editor_id: Option<egui::Id>,
    /// Fraction of the shared editor/results split given to the SQL editor (0.0–1.0).
    pub results_split_ratio: f32,
    pub expanded_schemas: std::collections::HashSet<usize>,
    /// Schema expansion scoped per database in browse-all mode: (db, schema_idx).
    pub expanded_db_schemas: std::collections::HashSet<(String, usize)>,
    pub expanded_connections: std::collections::HashSet<String>,
    /// Wall-clock time the last query was dispatched; consumed (taken) once
    /// the promise resolves, to compute `last_run_duration`.
    pub last_run_started_at: Option<std::time::Instant>,
    /// How long the most recently finished query took, success or failure.
    pub last_run_duration: Option<std::time::Duration>,
    /// Queries dispatched this session (process lifetime, not persisted).
    pub session_query_count: u32,
    /// Queries that finished in error this session.
    pub session_failed_count: u32,
    /// Structured detail for the most recent failure; cleared once a new
    /// query is dispatched, a query succeeds, or the connection changes.
    pub last_error: Option<crate::db::QueryError>,
    /// Whether the error panel's full-detail view is open.
    pub error_detail_expanded: bool,
    /// Whether EXPLAIN runs with `ANALYZE` (executes the statement for real
    /// timings); off by default and refused for data-modifying statements.
    pub explain_analyze: bool,
    /// Plan render format: pretty (indented JSON, default) vs raw
    /// (byte-for-byte as Postgres returned it).
    pub explain_pretty: bool,
    /// The last successful EXPLAIN result; `None` hides the plan panel.
    pub explain_result: Option<db::ExplainResult>,
    /// Whether the plan panel is expanded (collapsed shows its header row).
    pub explain_panel_open: bool,
    /// In-flight EXPLAIN task: resolves to `(plan, analyze)`. It reuses the
    /// dispatch/duration bookkeeping of normal queries but lands in
    /// `explain_result` instead of `result`.
    pub explain_promise: Option<Promise<anyhow::Result<(String, bool)>>>,
    /// Abort handle for the running EXPLAIN task (cancellation).
    pub explain_cancel: Option<tokio::task::AbortHandle>,
    /// Results grid selection `(row, column)` (0-based, into the rows as
    /// displayed this frame). Currently drives copy; reserved as the anchor
    /// for cell editing in a later release.
    pub selected_cell: Option<(usize, usize)>,
    /// Transient "copied cell" feedback shown in the results panel.
    pub copy_feedback: Option<CopyFeedback>,
    /// Modal cell-edit dialog (opened from the results grid context menu).
    pub edit: EditDialog,
    /// Modal full-row edit dialog (opened from the results grid context menu).
    pub row_edit: RowEditDialog,
    /// Installed extensions of the connected database (single-db mode).
    pub extensions: Vec<db::ExtensionInfo>,
    pub extension_promise: Option<Promise<anyhow::Result<Vec<db::ExtensionInfo>>>>,
    /// `(database, schema, table)` triples whose detail sub-list is expanded
    /// in the tree. `database` is `None` in single-database mode, so the same
    /// schema/table name across two databases in browse-all mode stays
    /// distinct (and its details load from the right pool).
    pub expanded_tables: std::collections::HashSet<(Option<String>, String, String)>,
    /// Lazy per-table detail load (indexes / fks / triggers), triggered by
    /// expanding a table node.
    pub table_detail_promise: Option<(Option<String>, String, String, TableDetailPromise)>,
    /// Modal "Delete schema" dialog state.
    pub drop_schema: Option<DropSchemaDialog>,
    /// Modal "New schema" / "Rename schema" dialog state.
    pub schema_name_dialog: Option<SchemaNameDialog>,
}

/// Short-lived notification that a cell value was copied to the clipboard;
/// auto-clears a couple of seconds after it is posted.
pub struct CopyFeedback {
    pub text: String,
    pub since: std::time::Instant,
}

/// Modal "Edit cell" dialog state (results grid → right-click → Edit cell).
/// Safe by construction: editing is only offered when the result set is a
/// plain projection of the currently selected table and that table has a
/// primary key, so the generated UPDATE always targets exactly one row.
pub struct EditDialog {
    pub open: bool,
    /// Row index into the current result set (stable across client-side
    /// filtering).
    pub row_idx: usize,
    /// Column index into the current result set being edited.
    pub col_idx: usize,
    pub schema: String,
    pub table: String,
    pub column: String,
    /// Type shown to the user (from the result set, e.g. "INT4").
    pub column_type: String,
    pub nullable: bool,
    /// `(primary-key column, value)` pairs used to build the WHERE clause.
    pub identity: Vec<(String, String)>,
    /// Text currently in the value box (starts as the current cell text).
    pub value: String,
    /// True while the UPDATE is in flight (Save disabled).
    pub saving: bool,
    /// Structured failure from the last save attempt.
    pub error: Option<crate::db::QueryError>,
    /// Wetther the dialog's error detail panel is expanded.
    pub detail_expanded: bool,
    pub promise: Option<Promise<anyhow::Result<u64>>>,
}

impl Default for EditDialog {
    fn default() -> Self {
        Self {
            open: false,
            row_idx: 0,
            col_idx: 0,
            schema: String::new(),
            table: String::new(),
            column: String::new(),
            column_type: String::new(),
            nullable: true,
            identity: Vec::new(),
            value: String::new(),
            saving: false,
            error: None,
            detail_expanded: false,
            promise: None,
        }
    }
}

/// One editable column in the full-row dialog. `dirty` only becomes true when
/// the user actually changed the value; only dirty fields make it into the
/// generated UPDATE's SET clause.
pub struct RowEditField {
    pub column: String,
    pub column_type: String,
    pub nullable: bool,
    pub original_value: String,
    pub value: String,
    pub dirty: bool,
}

/// Modal "Edit row" dialog state (results grid right-click → Edit row).
/// Like [`EditDialog`], it is only offered when the result set is a plain
/// projection of the selected table with a primary key in the result, so the
/// generated UPDATE always targets exactly one row.
#[derive(Default)]
pub struct RowEditDialog {
    pub open: bool,
    /// Row index into the current result set (stable across client-side
    /// filtering).
    pub row_idx: usize,
    pub schema: String,
    pub table: String,
    /// `(primary-key column, value)` pairs used to build the WHERE clause.
    /// Shown read-only under "Row identity"; never editable.
    pub identity: Vec<(String, String)>,
    /// Non-PK result columns, one entry per editable field.
    pub fields: Vec<RowEditField>,
    /// True while the UPDATE is in flight (Save disabled).
    pub saving: bool,
    /// Structured failure from the last save attempt.
    pub error: Option<crate::db::QueryError>,
    /// Whether the dialog's error detail panel is expanded.
    pub detail_expanded: bool,
    pub promise: Option<Promise<anyhow::Result<u64>>>,
}

/// Modal "Delete schema" dialog state.
#[derive(Default)]
pub struct DropSchemaDialog {
    /// Some(db) in browse-all mode, None in single-database mode.
    pub database: Option<String>,
    pub schema: String,
    pub table_count: usize,
    pub confirm_text: String,
    pub cascade: bool,
    pub error: Option<String>,
    pub promise: Option<Promise<anyhow::Result<()>>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SchemaNameMode {
    Create,
    Rename,
    /// CREATE DATABASE on the connected server (see `open_create_database`).
    CreateDatabase,
}

/// Modal dialog shared by "New schema", "Rename schema" and "New database".
pub struct SchemaNameDialog {
    pub mode: SchemaNameMode,
    /// Some(db) in browse-all mode, None in single-database mode (and for
    /// CreateDatabase, which always runs against the active pool).
    pub database: Option<String>,
    /// Source database to clone (CreateDatabase only): `CREATE DATABASE new
    /// TEMPLATE old`.
    pub template: Option<String>,
    /// Existing name (Rename only; empty for Create).
    pub old_name: String,
    pub name: String,
    pub focused: bool,
    pub error: Option<String>,
    pub promise: Option<Promise<anyhow::Result<()>>>,
}

impl Default for QueryState {
    fn default() -> Self {
        Self {
            schemas: Vec::new(),
            databases: Vec::new(),
            database_pools: std::collections::HashMap::new(),
            expanded_databases: std::collections::HashSet::new(),
            active_database: None,
            schema_idx: 0,
            table_idx: 0,
            focus: Focus::Schemas,
            sql: String::new(),
            result: None,
            status: String::new(),
            schema_promise: None,
            query_promise: None,
            database_promise: None,
            db_schema_promise: None,
            limit_value: "100".into(),
            offset_value: "0".into(),
            current_page: 0,
            has_next_page: false,
            filter_text: String::new(),
            tree_filter: String::new(),
            pending_confirm_sql: None,
            query_cancel: None,
            autocomplete_matches: Vec::new(),
            autocomplete_splice: None,
            autocomplete_open: false,
            autocomplete_selected: 0,
            pending_caret: None,
            editor_id: None,
            results_split_ratio: 0.5,
            expanded_schemas: std::collections::HashSet::new(),
            expanded_db_schemas: std::collections::HashSet::new(),
            expanded_connections: std::collections::HashSet::new(),
            last_run_started_at: None,
            last_run_duration: None,
            session_query_count: 0,
            session_failed_count: 0,
            last_error: None,
            error_detail_expanded: false,
            explain_analyze: false,
            explain_pretty: true,
            explain_result: None,
            explain_panel_open: true,
            explain_promise: None,
            explain_cancel: None,
            selected_cell: None,
            copy_feedback: None,
            edit: EditDialog::default(),
            row_edit: RowEditDialog::default(),
            drop_schema: None,
            schema_name_dialog: None,
            extensions: Vec::new(),
            extension_promise: None,
            expanded_tables: std::collections::HashSet::new(),
            table_detail_promise: None,
        }
    }
}

impl QueryState {
    /// Call exactly once, right before spawning a query task.
    pub fn record_query_dispatch(&mut self) {
        self.session_query_count += 1;
        self.last_run_started_at = Some(std::time::Instant::now());
        self.last_error = None;
        self.error_detail_expanded = false;
    }

    /// Call in the `Ok` arm of the query-promise poll.
    pub fn record_query_success(&mut self) {
        if let Some(started) = self.last_run_started_at.take() {
            self.last_run_duration = Some(started.elapsed());
        }
    }

    /// Call in the `Err` arm of the query-promise poll.
    pub fn record_query_failure(&mut self, err: &anyhow::Error) {
        if let Some(started) = self.last_run_started_at.take() {
            self.last_run_duration = Some(started.elapsed());
        }
        self.session_failed_count += 1;
        self.last_error = Some(crate::db::QueryError::from_anyhow(err));
    }
}

pub struct AppState {
    pub connection: ConnectionState,
    pub query: QueryState,
    pub connect_promise: Option<Promise<Result<PgPool, anyhow::Error>>>,
    /// Shared tokio runtime; query tasks spawn here so they can be aborted.
    pub rt: std::sync::Arc<tokio::runtime::Runtime>,
}

impl AppState {
    pub fn new(rt: std::sync::Arc<tokio::runtime::Runtime>) -> Self {
        let secrets = Arc::new(crate::services::VaultStore::new()) as Arc<dyn SecretStore>;
        let repo = ProfileRepo::new(secrets.clone());
        let (profiles, warnings) = repo.load().unwrap_or_else(|e| {
            tracing::error!("Failed to load profiles: {}", e);
            (
                Vec::new(),
                vec![format!("Failed to load connections: {}", e)],
            )
        });
        for w in &warnings {
            tracing::warn!("{}", w);
        }

        Self {
            connection: ConnectionState {
                profiles,
                active_id: None,
                active_pool: None,
                active_password: None,
                dialog: ConnectionDialog::default(),
                password_prompt: PasswordPrompt::default(),
                repo,
                secrets,
                menu_open: false,
            },
            query: QueryState::default(),
            connect_promise: None,
            rt,
        }
    }

    pub fn active_pool(&self) -> Option<&PgPool> {
        self.connection.active_pool.as_ref()
    }

    pub fn is_connected(&self) -> bool {
        self.connection.active_pool.is_some()
    }

    pub fn start_load_schemas(&mut self) {
        if let Some(pool) = &self.connection.active_pool {
            let pool = pool.clone();
            self.query.schema_promise = Some(Promise::spawn_async(async move {
                db::load_schemas(&pool).await
            }));
            self.query.status = "Loading schemas...".into();
        }
    }

    /// True when the active connection is in "browse all databases" mode.
    pub fn browse_all_active(&self) -> bool {
        self.connection
            .active_id
            .as_ref()
            .and_then(|id| self.connection.profiles.iter().find(|p| &p.id == id))
            .is_some_and(|p| p.browse_all_databases())
    }

    /// Called instead of start_load_schemas() when the active profile is in
    /// "browse all databases" mode.
    pub fn start_load_databases(&mut self) {
        if let Some(pool) = &self.connection.active_pool {
            let pool = pool.clone();
            self.query.database_promise = Some(Promise::spawn_async(async move {
                db::list_databases(&pool).await
            }));
            self.query.status = "Loading databases...".into();
        }
    }

    /// After connecting, load either the database list or the schema list,
    /// depending on the profile's mode.
    pub fn start_post_connect_load(&mut self) {
        if self.browse_all_active() {
            self.start_load_databases();
        } else {
            self.start_load_schemas();
        }
    }

    /// Open the "Delete schema" dialog for a schema in the given database.
    /// `database` is None in single-database mode, Some(name) in browse-all mode.
    pub fn open_drop_schema(&mut self, database: Option<&str>, schema: &str, table_count: usize) {
        if crate::db::is_protected_schema(schema) {
            return;
        }
        self.query.drop_schema = Some(DropSchemaDialog {
            database: database.map(str::to_string),
            schema: schema.to_string(),
            table_count,
            confirm_text: String::new(),
            cascade: false,
            error: None,
            promise: None,
        });
    }

    /// Confirm the schema deletion (called when user clicks "Delete schema" in the dialog).
    pub fn confirm_drop_schema(&mut self) {
        let pool = match self.query.drop_schema.as_ref() {
            Some(d) => match &d.database {
                Some(name) => self.query.database_pools.get(name).cloned(),
                None => self.connection.active_pool.clone(),
            },
            None => return,
        };
        let Some(d) = self.query.drop_schema.as_mut() else {
            return;
        };
        if d.promise.is_some() || d.confirm_text != d.schema {
            return;
        }
        let Some(pool) = pool else {
            d.error = Some("No open connection for this database".into());
            return;
        };
        d.error = None;
        let (schema, cascade) = (d.schema.clone(), d.cascade);
        d.promise = Some(Promise::spawn_async(async move {
            crate::db::drop_schema(&pool, &schema, cascade).await
        }));
    }

    /// Poll the in-flight schema deletion. Returns true when the UI should repaint.
    pub fn poll_drop_schema(&mut self) -> bool {
        let outcome: Result<(), String> = match self
            .query
            .drop_schema
            .as_ref()
            .and_then(|d| d.promise.as_ref())
        {
            Some(p) => match p.ready() {
                None => return false,
                Some(Ok(())) => Ok(()),
                Some(Err(e)) => Err(format!("{e:#}")),
            },
            None => return false,
        };
        match outcome {
            Err(msg) => {
                if let Some(d) = self.query.drop_schema.as_mut() {
                    d.promise = None;
                    d.error = Some(msg);
                }
            }
            Ok(()) => {
                let Some(d) = self.query.drop_schema.take() else {
                    return true;
                };
                // Schema indices shift after a drop: reset selection and reload.
                self.query.schema_idx = 0;
                self.query.table_idx = 0;
                self.query.result = None;
                match &d.database {
                    Some(name) => {
                        self.query.expanded_db_schemas.retain(|(db, _)| db != name);
                        if let Some(info) =
                            self.query.databases.iter_mut().find(|x| x.name == *name)
                        {
                            info.loaded = false;
                        }
                        self.expand_database(name);
                    }
                    None => {
                        self.query.expanded_schemas.clear();
                        self.start_load_schemas();
                    }
                }
                self.query.status = format!("Schema \"{}\" dropped", d.schema);
            }
        }
        true
    }

    /// Pool for the given database (browse-all) or the single active pool.
    fn schema_pool(&self, database: Option<&str>) -> Option<PgPool> {
        match database {
            Some(name) => self.query.database_pools.get(name).cloned(),
            None => self.connection.active_pool.clone(),
        }
    }

    /// Reload the schema list after a schema was created / renamed / dropped
    /// (or on manual refresh). Schema indices shift, so selection and
    /// expansion state tied to those indices is reset.
    pub fn reload_schemas(&mut self, database: Option<&str>) {
        self.query.schema_idx = 0;
        self.query.table_idx = 0;
        self.query.result = None;
        match database {
            Some(name) => {
                self.query.expanded_db_schemas.retain(|(db, _)| db != name);
                if let Some(info) = self.query.databases.iter_mut().find(|x| x.name == name) {
                    info.loaded = false;
                }
                self.expand_database(name);
            }
            None => {
                self.query.expanded_schemas.clear();
                self.start_load_schemas();
            }
        }
    }

    /// Manual refresh of the schema list (this database in browse-all mode).
    pub fn refresh_schemas(&mut self, database: Option<&str>) {
        self.reload_schemas(database);
    }

    pub fn open_create_schema(&mut self, database: Option<&str>) {
        self.query.schema_name_dialog = Some(SchemaNameDialog {
            mode: SchemaNameMode::Create,
            database: database.map(str::to_string),
            template: None,
            old_name: String::new(),
            name: String::new(),
            focused: false,
            error: None,
            promise: None,
        });
    }

    pub fn open_rename_schema(&mut self, database: Option<&str>, schema: &str) {
        if crate::db::is_system_schema(schema) {
            return;
        }
        self.query.schema_name_dialog = Some(SchemaNameDialog {
            mode: SchemaNameMode::Rename,
            database: database.map(str::to_string),
            template: None,
            old_name: schema.to_string(),
            name: schema.to_string(),
            focused: false,
            error: None,
            promise: None,
        });
    }

    /// Prompt for a new database name on the currently connected server.
    /// `template = Some(db)` clones that database (`CREATE DATABASE … TEMPLATE …`).
    pub fn open_create_database(&mut self, template: Option<&str>) {
        self.query.schema_name_dialog = Some(SchemaNameDialog {
            mode: SchemaNameMode::CreateDatabase,
            database: None,
            template: template.map(str::to_string),
            old_name: String::new(),
            name: String::new(),
            focused: false,
            error: None,
            promise: None,
        });
    }

    pub fn confirm_schema_name(&mut self) {
        let pool = match self.query.schema_name_dialog.as_ref() {
            Some(d) => self.schema_pool(d.database.as_deref()),
            None => return,
        };
        let Some(d) = self.query.schema_name_dialog.as_mut() else {
            return;
        };
        if d.promise.is_some() {
            return;
        }
        let name = d.name.trim().to_string();
        if d.mode == SchemaNameMode::CreateDatabase {
            if let Err(e) = validate_database_name(&name) {
                d.error = Some(e.to_string());
                return;
            }
        } else if let Err(e) = crate::db::validate_schema_name(&name) {
            d.error = Some(e.to_string());
            return;
        }
        if d.mode == SchemaNameMode::Rename && name == d.old_name {
            d.error = Some("Name is unchanged".into());
            return;
        }
        let Some(pool) = pool else {
            d.error = Some(
                if d.mode == SchemaNameMode::CreateDatabase {
                    "No active connection to the server"
                } else {
                    "No open connection for this database"
                }
                .into(),
            );
            return;
        };
        d.error = None;
        let (mode, old, template) = (d.mode, d.old_name.clone(), d.template.clone());
        d.promise = Some(Promise::spawn_async(async move {
            match mode {
                SchemaNameMode::Create => crate::db::create_schema(&pool, &name).await,
                SchemaNameMode::Rename => crate::db::rename_schema(&pool, &old, &name).await,
                SchemaNameMode::CreateDatabase => {
                    let sql = match template {
                        Some(src) => format!(
                            "CREATE DATABASE {} TEMPLATE {}",
                            crate::db::query::quote_ident(&name),
                            crate::db::query::quote_ident(&src)
                        ),
                        None => format!("CREATE DATABASE {}", crate::db::query::quote_ident(&name)),
                    };
                    sqlx::raw_sql(&sql)
                        .execute(&pool)
                        .await
                        .map_err(|e| anyhow::anyhow!("CREATE DATABASE \"{name}\" failed: {e}"))?;
                    Ok(())
                }
            }
        }));
    }

    /// Poll the in-flight create/rename. Returns true when the UI should repaint.
    pub fn poll_schema_name(&mut self) -> bool {
        let outcome: Result<(), String> = match self
            .query
            .schema_name_dialog
            .as_ref()
            .and_then(|d| d.promise.as_ref())
        {
            Some(p) => match p.ready() {
                None => return false,
                Some(Ok(())) => Ok(()),
                Some(Err(e)) => Err(format!("{e:#}")),
            },
            None => return false,
        };
        match outcome {
            Err(msg) => {
                if let Some(d) = self.query.schema_name_dialog.as_mut() {
                    d.promise = None;
                    d.error = Some(msg);
                }
            }
            Ok(()) => {
                let Some(d) = self.query.schema_name_dialog.take() else {
                    return true;
                };
                if d.mode == SchemaNameMode::CreateDatabase {
                    self.query.status = format!("Database \"{}\" created", d.name.trim());
                    if self.browse_all_active() {
                        self.start_load_databases();
                    }
                } else {
                    self.reload_schemas(d.database.as_deref());
                    let verb = match d.mode {
                        SchemaNameMode::Create => "created",
                        SchemaNameMode::Rename => "renamed",
                        SchemaNameMode::CreateDatabase => unreachable!(),
                    };
                    self.query.status = format!("Schema \"{}\" {verb}", d.name.trim());
                }
            }
        }
        true
    }

    /// Called when the user expands a database node in the sidebar tree.
    /// Lazily opens a second connection scoped to that database and loads
    /// its schemas; reuses an already-open pool on subsequent expands.
    pub fn expand_database(&mut self, db_name: &str) {
        let already_loaded = self
            .query
            .databases
            .iter()
            .find(|d| d.name == db_name)
            .map(|d| d.loaded)
            .unwrap_or(false);
        if already_loaded {
            return;
        }
        // Only one lazy connect/reload at a time — don't clobber an in-flight one.
        if let Some((pending_name, promise)) = &self.query.db_schema_promise
            && (pending_name == db_name || promise.ready().is_none())
        {
            return;
        }

        if let Some(pool) = self.query.database_pools.get(db_name).cloned() {
            let db_name_owned = db_name.to_string();
            self.query.status = format!("Loading schemas for {db_name}...");
            self.query.db_schema_promise = Some((
                db_name_owned,
                Promise::spawn_async(async move {
                    let schemas = db::load_schemas(&pool).await?;
                    Ok((pool, schemas))
                }),
            ));
            return;
        }

        let Some(profile_id) = self.connection.active_id.clone() else {
            return;
        };
        let Some(profile) = self
            .connection
            .profiles
            .iter()
            .find(|p| p.id == profile_id)
            .cloned()
        else {
            return;
        };

        let password = self.connection.active_password.clone().unwrap_or_default();
        let url = profile.connection_url_for_database(&password, db_name);
        let db_name_owned = db_name.to_string();
        self.query.status = format!("Connecting to {}...", db_name);
        self.query.db_schema_promise = Some((
            db_name_owned,
            Promise::spawn_async(async move {
                let pool = db::connect_with_url(&url).await?;
                let schemas = db::load_schemas(&pool).await?;
                Ok((pool, schemas))
            }),
        ));
    }

    pub fn set_active_pool(&mut self, pool: PgPool) {
        self.connection.active_pool = Some(pool);
        self.start_post_connect_load();
    }

    pub fn switch_connection(&mut self, profile_id: &str) {
        if self.connection.active_id.as_deref() == Some(profile_id) {
            return;
        }
        if let Some(profile) = self.connection.profiles.iter().find(|p| p.id == profile_id) {
            let profile = profile.clone();
            // Try to get password from keyring
            match self.connection.secrets.get(&profile.id) {
                Ok(Some(password)) => {
                    self.start_connect(profile, password);
                }
                Ok(None) => {
                    // No saved password - open password prompt
                    self.connection.password_prompt.open(&profile);
                }
                Err(e) => {
                    self.query.status = format!("Cannot read saved password: {}", e);
                }
            }
        }
    }

    /// Start async connection with a known password.
    fn start_connect(&mut self, profile: ConnectionProfile, password: String) {
        let profile_id = profile.id.clone();
        let profile_name = profile.name.clone();
        let pw_for_task = password.clone();
        let pool_promise = poll_promise::Promise::spawn_async(async move {
            db::connect_server(&profile, &pw_for_task).await
        });
        self.query.status = format!("Connecting to {}...", profile_name);
        self.connection.active_id = Some(profile_id.clone());
        self.connection.active_password = Some(password);
        self.query.expanded_connections.insert(profile_id);
        self.clear_query_state();
        self.connect_promise = Some(pool_promise);
    }

    /// Check if a profile with same host+port+database+username exists (different id).
    fn find_duplicate(&self, profile: &ConnectionProfile) -> Option<(String, String)> {
        self.connection
            .profiles
            .iter()
            .find(|p| {
                p.id != profile.id
                    && p.host == profile.host
                    && p.port == profile.port
                    && p.database.as_deref().unwrap_or("")
                        == profile.database.as_deref().unwrap_or("")
                    && p.username == profile.username
            })
            .map(|p| (p.id.clone(), p.generated_name()))
    }

    /// Upsert profile, save via repo, then connect.
    /// Returns `true` when the profile was persisted and the connection
    /// finalized; `false` when the repo save failed (caller should keep the
    /// dialog open so the user can retry).
    fn upsert_profile_and_connect(
        &mut self,
        profile: ConnectionProfile,
        password: String,
        pool: PgPool,
    ) -> bool {
        // Upsert
        if let Some(existing) = self
            .connection
            .profiles
            .iter_mut()
            .find(|p| p.id == profile.id)
        {
            *existing = profile.clone();
        } else {
            self.connection.profiles.push(profile.clone());
        }
        // Save via repo (surfaces errors)
        if let Err(e) = self.connection.repo.save(&self.connection.profiles) {
            self.query.status = format!("Failed to save profile: {}", e);
            return false;
        }
        // Connect
        self.finish_connect(profile.id, password, pool);
        true
    }

    /// Finalize connection after profile saved (no duplicate or user confirmed).
    fn finish_connect(&mut self, profile_id: String, password: String, pool: PgPool) {
        self.connection.active_id = Some(profile_id.clone());
        self.connection.active_password = Some(password);
        self.connection.active_pool = Some(pool);
        self.query.expanded_connections.insert(profile_id);
        self.clear_query_state();
        self.start_post_connect_load();
        self.query.status = "Connected".into();
    }

    /// Commit pending duplicate save (user clicked "Save anyway").
    fn commit_pending_save(&mut self, profile: ConnectionProfile, pool: PgPool) -> bool {
        let password = self.connection.dialog.password.clone();
        self.upsert_profile_and_connect(profile, password, pool)
    }

    /// Called when user confirms "Save anyway" in the dialog.
    pub fn confirm_duplicate_save(&mut self) {
        if let Some((_warn, profile, pool)) = self.connection.dialog.pending_duplicate.take() {
            if self.upsert_profile_and_connect(
                profile,
                self.connection.dialog.password.clone(),
                pool,
            ) {
                self.connection.dialog.close();
            }
        }
    }

    /// Submit password from password prompt (test connection).
    pub fn submit_password_prompt(&mut self) {
        let prompt = &mut self.connection.password_prompt;
        if prompt.submitting {
            return;
        }
        prompt.error = None;
        prompt.submitting = true;
        // Find the profile to get the URL for testing
        if let Some(profile) = self
            .connection
            .profiles
            .iter()
            .find(|p| p.id == prompt.profile_id)
            .cloned()
        {
            let password = prompt.password.clone();
            prompt.test_promise = Some(Promise::spawn_async(async move {
                db::connect_server(&profile, &password).await
            }));
        } else {
            prompt.error = Some("Profile not found".into());
            prompt.submitting = false;
        }
    }

    /// Cancel password prompt.
    pub fn cancel_password_prompt(&mut self) {
        self.connection.password_prompt.close();
    }

    /// Open the edit dialog for a profile, fetching the password from the secret store.
    pub fn open_edit_profile(&mut self, profile_id: &str) {
        if let Some(profile) = self
            .connection
            .profiles
            .iter()
            .find(|p| p.id == profile_id)
            .cloned()
        {
            let password = self
                .connection
                .secrets
                .get(&profile.id)
                .ok()
                .flatten()
                .unwrap_or_default();
            self.connection.dialog.open_edit(&profile, &password);
        }
    }

    pub fn clear_query_state(&mut self) {
        // Close all per-database pools to avoid leaking open connections.
        for (_, pool) in self.query.database_pools.drain() {
            tokio::spawn(async move { pool.close().await });
        }
        self.query.schemas.clear();
        self.query.databases.clear();
        self.query.expanded_databases.clear();
        self.query.extensions.clear();
        self.query.extension_promise = None;
        self.query.expanded_tables.clear();
        self.query.table_detail_promise = None;
        self.query.active_database = None;
        self.query.schema_idx = 0;
        self.query.table_idx = 0;
        self.query.sql.clear();
        self.query.result = None;
        self.close_autocomplete();
        self.query.schema_promise = None;
        self.query.database_promise = None;
        self.query.db_schema_promise = None;
        self.query.query_promise = None;
        self.query.query_cancel = None;
        self.query.pending_confirm_sql = None;
        self.query.current_page = 0;
        self.query.has_next_page = false;
        self.query.filter_text.clear();
        self.query.expanded_schemas.clear();
        self.query.expanded_db_schemas.clear();
        // The error panel describes a failure against the *old* connection;
        // drop it on switch. Session counters are process-scoped and stay.
        self.query.last_error = None;
        self.query.error_detail_expanded = false;
        self.query.last_run_started_at = None;
        self.query.explain_result = None;
        self.query.explain_panel_open = true;
        self.query.explain_promise = None;
        self.query.explain_cancel = None;
    }

    pub fn poll_promises(&mut self) -> bool {
        let mut changed = false;

        // Poll connect promise
        if let Some(p) = &self.connect_promise {
            match p.ready() {
                None => {}
                Some(Ok(pool)) => {
                    let pool = pool.clone();
                    self.connection.active_pool = Some(pool);
                    self.connect_promise = None;
                    self.start_post_connect_load();
                    changed = true;
                }
                Some(Err(e)) => {
                    self.query.status = format!("Connection failed: {e}");
                    self.connection.active_id = None;
                    self.connection.active_password = None;
                    self.connect_promise = None;
                    changed = true;
                }
            }
        }

        // Poll password prompt
        if self.connection.password_prompt.submitting {
            if let Some(promise) = self.connection.password_prompt.test_promise.take() {
                match promise.ready() {
                    None => {
                        self.connection.password_prompt.test_promise = Some(promise);
                    }
                    Some(Ok(pool)) => {
                        let pool = pool.clone();
                        // Extract values from prompt before mutably borrowing self for method calls
                        let profile_id = self.connection.password_prompt.profile_id.clone();
                        let password = self.connection.password_prompt.password.clone();
                        let pending_duplicate =
                            self.connection.password_prompt.pending_duplicate.take();
                        let remember = self.connection.password_prompt.remember;
                        let prompt_ptr: *mut PasswordPrompt =
                            &mut self.connection.password_prompt as *mut _;
                        // Save secret if remember checked
                        if remember {
                            if let Err(e) = self.connection.secrets.set(&profile_id, &password) {
                                // Need to re-borrow to set error
                                unsafe {
                                    (*prompt_ptr).error =
                                        Some(format!("Failed to save password: {}", e));
                                }
                                unsafe {
                                    (*prompt_ptr).submitting = false;
                                }
                                changed = true;
                            } else {
                                // Secret saved, now commit pending duplicate if any
                                let saved =
                                    if let Some((_warn, profile, pool)) = pending_duplicate {
                                        self.commit_pending_save(profile, pool)
                                    } else {
                                        self.finish_connect(profile_id, password, pool);
                                        true
                                    };
                                unsafe {
                                    if saved {
                                        (*prompt_ptr).close();
                                    } else {
                                        (*prompt_ptr).submitting = false;
                                    }
                                }
                                changed = true;
                            }
                        } else {
                            // Not remembering, just connect
                            let saved = if let Some((_warn, profile, pool)) = pending_duplicate {
                                self.commit_pending_save(profile, pool)
                            } else {
                                self.finish_connect(profile_id, password, pool);
                                true
                            };
                            unsafe {
                                if saved {
                                    (*prompt_ptr).close();
                                } else {
                                    (*prompt_ptr).submitting = false;
                                }
                            }
                            changed = true;
                        }
                    }
                    Some(Err(e)) => {
                        self.connection.password_prompt.error = Some(format!("{e}"));
                        self.connection.password_prompt.submitting = false;
                        changed = true;
                    }
                }
            }
        }

        // Poll dialog test promise
        if let Some(p) = &self.connection.dialog.test_promise {
            match p.ready() {
                None => {}
                Some(Ok(pool)) => {
                    let pool = pool.clone();
                    // Clear the promise immediately so this branch only runs once.
                    self.connection.dialog.test_promise = None;
                    let profile = self.connection.dialog.build_profile();
                    let password = self.connection.dialog.password.clone();

                    // Save secret FIRST
                    if let Err(e) = self.connection.secrets.set(&profile.id, &password) {
                        self.connection.dialog.error =
                            Some(format!("Failed to save password: {}", e));
                        self.connection.dialog.testing = false;
                        changed = true;
                    } else {
                        // Secret saved. Check for duplicate (same host+port+db+user, different id).
                        let duplicate = self.find_duplicate(&profile);
                        if let Some((_other_id, other_name)) = duplicate {
                            // Store pending and show warning in dialog
                            self.connection.dialog.pending_duplicate = Some((
                                format!(
                                    "Same server already saved as \"{}\". Save anyway?",
                                    other_name
                                ),
                                profile,
                                pool,
                            ));
                            self.connection.dialog.testing = false;
                        } else if self.upsert_profile_and_connect(profile, password, pool) {
                            // No duplicate, saved and connected: dismiss the dialog.
                            // (`close()` also resets `testing`.)
                            self.connection.dialog.close();
                        } else {
                            // Save failed (status already carries the error):
                            // re-enable the button so the user can retry.
                            self.connection.dialog.testing = false;
                        }
                        changed = true;
                    }
                }
                Some(Err(e)) => {
                    self.connection.dialog.error = Some(format!("{e}"));
                    self.connection.dialog.testing = false;
                    self.connection.dialog.test_promise = None;
                    changed = true;
                }
            }
        }

        // Poll database list promise (browse-all mode)
        if let Some(p) = &self.query.database_promise {
            match p.ready() {
                None => {}
                Some(Ok(databases)) => {
                    self.query.databases = databases.clone();
                    self.query.database_promise = None;
                    self.query.status =
                        format!("Connected — {} database(s)", self.query.databases.len());
                    changed = true;
                }
                Some(Err(e)) => {
                    self.query.status = format!("Database list error: {e}");
                    self.query.database_promise = None;
                    changed = true;
                }
            }
        }

        // Poll per-database connect+schemas promise (browse-all mode)
        let mut db_loaded: Option<(String, PgPool, Vec<db::SchemaInfo>)> = None;
        if let Some((name, p)) = &self.query.db_schema_promise {
            match p.ready() {
                None => {}
                Some(Ok((pool, schemas))) => {
                    db_loaded = Some((name.clone(), pool.clone(), schemas.clone()));
                }
                Some(Err(e)) => {
                    self.query.status = format!("Failed to load {name}: {e}");
                    self.query.db_schema_promise = None;
                    changed = true;
                }
            }
        }
        if let Some((name, pool, schemas)) = db_loaded {
            self.query.database_pools.insert(name.clone(), pool);
            if let Some(info) = self.query.databases.iter_mut().find(|d| d.name == name) {
                info.schemas = schemas;
                info.loaded = true;
            }
            self.query.db_schema_promise = None;
            // First opened database becomes the default query target.
            if self.query.active_database.is_none() {
                self.query.active_database = Some(name.clone());
            }
            self.update_sql();
            self.query.status = format!("{name} — schemas loaded");
            // Extensions are database-level; load them for this database too.
            if let Some(pool) = self.query.database_pools.get(&name).cloned() {
                self.spawn_extension_load(pool);
            }
            changed = true;
        }

        // Poll schema promise
        if let Some(p) = &self.query.schema_promise {
            match p.ready() {
                None => {}
                Some(Ok(schemas)) => {
                    self.query.schemas = schemas.clone();
                    self.query.schema_promise = None;
                    self.update_sql();
                    self.query.status =
                        format!("Connected — {} schema(s) loaded", self.query.schemas.len());
                    if let Some(pool) = self.query_pool() {
                        self.spawn_extension_load(pool);
                    }
                    changed = true;
                }
                Some(Err(e)) => {
                    self.query.status = format!("Schema load error: {e}");
                    self.query.schema_promise = None;
                    changed = true;
                }
            }
        }

        // Poll extension promise
        if let Some(p) = &self.query.extension_promise {
            match p.ready() {
                None => {}
                Some(Ok(extensions)) => {
                    self.query.extensions = extensions.clone();
                    self.query.extension_promise = None;
                    changed = true;
                }
                Some(Err(e)) => {
                    self.query.status = format!("Extension load error: {e}");
                    self.query.extension_promise = None;
                    changed = true;
                }
            }
        }

        // Poll per-table detail promise (lazy indexes / fks / triggers).
        if let Some((db, schema, table, p)) = self.query.table_detail_promise.take() {
            match p.ready() {
                None => {
                    self.query.table_detail_promise = Some((db, schema, table, p));
                }
                Some(Ok(details)) => {
                    if let Some(t) = self.find_table_mut(db.as_deref(), &schema, &table) {
                        t.indexes = details.indexes.clone();
                        t.foreign_keys = details.foreign_keys.clone();
                        t.triggers = details.triggers.clone();
                        t.detail_loaded = true;
                    }
                    changed = true;
                }
                Some(Err(e)) => {
                    self.query.status = format!("Failed to load {schema}.{table} details: {e}");
                    changed = true;
                }
            }
        }

        // Poll query promise. Taken out so the record_* helpers (which take
        // &mut QueryState) don't fight the in-progress borrow of the field;
        // restored immediately when still pending.
        if let Some(p) = self.query.query_promise.take() {
            match p.ready() {
                None => {
                    self.query.query_promise = Some(p);
                }
                Some(Ok(result)) => {
                    self.query.record_query_success();
                    let rows = result.rows.len();
                    // A full page means there are probably more rows after it.
                    let limit: u64 = self.query.limit_value.parse().unwrap_or(100);
                    self.query.has_next_page = rows as u64 == limit;
                    // The result set was replaced, so cell coordinates and
                    // any copy feedback no longer refer to the same data.
                    self.query.selected_cell = None;
                    self.query.copy_feedback = None;
                    self.query.result = Some(result.clone());
                    self.query.status = format!(
                        "{rows} rows returned \u{2014} page {}",
                        self.query.current_page + 1
                    );
                    changed = true;
                }
                Some(Err(e)) => {
                    self.query.record_query_failure(e);
                    self.query.status = format!("Query error: {e}");
                    changed = true;
                }
            }
        }

        // Poll EXPLAIN promise (shares the normal query's dispatch bookkeeping).
        if let Some(p) = self.query.explain_promise.take() {
            match p.ready() {
                None => {
                    self.query.explain_promise = Some(p);
                }
                Some(Ok((plan, analyze))) => {
                    self.query.explain_cancel = None;
                    self.query.record_query_success();
                    let elapsed_ms = self
                        .query
                        .last_run_duration
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    self.query.explain_result = Some(db::ExplainResult {
                        plan: plan.clone(),
                        analyze: *analyze,
                        elapsed_ms,
                    });
                    self.query.explain_panel_open = true;
                    self.query.status = if *analyze {
                        format!("EXPLAIN ANALYZE complete \u{2014} {elapsed_ms} ms")
                    } else {
                        format!("EXPLAIN complete \u{2014} {elapsed_ms} ms")
                    };
                    changed = true;
                }
                Some(Err(e)) => {
                    self.query.explain_cancel = None;
                    self.query.record_query_failure(e);
                    self.query.status = format!("EXPLAIN error: {e}");
                    changed = true;
                }
            }
        }

        // Handle "Save anyway" confirmation from connection dialog duplicate warning.
        if self.connection.dialog.confirm_duplicate_save_requested {
            self.connection.dialog.confirm_duplicate_save_requested = false;
            self.confirm_duplicate_save();
            changed = true;
        }

        // Poll drop schema promise
        if self.poll_drop_schema() {
            changed = true;
        }

        // Poll schema name dialog promise (create/rename)
        if self.poll_schema_name() {
            changed = true;
        }

        changed
    }

    pub fn is_loading(&self) -> bool {
        self.connect_promise.is_some()
            || self.query.schema_promise.is_some()
            || self.query.database_promise.is_some()
            || self.query.db_schema_promise.is_some()
            || self.query.query_promise.is_some()
            || self.query.explain_promise.is_some()
            || self
                .query
                .drop_schema
                .as_ref()
                .is_some_and(|d| d.promise.is_some())
            || self
                .query
                .schema_name_dialog
                .as_ref()
                .is_some_and(|d| d.promise.is_some())
    }

    /// Kick off the database-level extension list for a connected pool.
    fn spawn_extension_load(&mut self, pool: PgPool) {
        self.query.extension_promise = Some(Promise::spawn_async(async move {
            db::load_extensions(&pool).await
        }));
    }

    /// Find a table by qualified name. `db` is `Some` when browsing multiple
    /// databases and scopes the search (plus the detail pool) to that
    /// database's tree.
    fn find_table_mut(
        &mut self,
        db: Option<&str>,
        schema: &str,
        table: &str,
    ) -> Option<&mut db::TableInfo> {
        if let Some(db) = db {
            return self
                .query
                .databases
                .iter_mut()
                .find(|d| d.name == db)
                .and_then(|d| {
                    d.schemas
                        .iter_mut()
                        .flat_map(|s| s.tables.iter_mut())
                        .find(|t| t.schema == schema && t.name == table)
                });
        }
        self.query
            .schemas
            .iter_mut()
            .flat_map(|s| s.tables.iter_mut())
            .find(|t| t.schema == schema && t.name == table)
    }

    /// Lazily fetch indexes / foreign keys / triggers for a table, triggered
    /// by expanding its node in the schema tree. No-op when already loaded or
    /// when a load is already in flight for that table.
    pub fn start_load_table_details(&mut self, db: Option<&str>, schema: &str, table: &str) {
        if self
            .find_table_mut(db, schema, table)
            .is_some_and(|t| t.detail_loaded)
        {
            return;
        }
        if let Some((d, s, t, _)) = &self.query.table_detail_promise
            && d.as_deref() == db
            && s == schema
            && t == table
        {
            return;
        }
        let pool = match db {
            Some(name) => match self.query.database_pools.get(name) {
                Some(pool) => pool.clone(),
                None => return,
            },
            None => match self.connection.active_pool {
                Some(ref pool) => pool.clone(),
                None => return,
            },
        };
        let schema = schema.to_string();
        let table = table.to_string();
        self.query.table_detail_promise = Some((
            db.map(str::to_owned),
            schema.clone(),
            table.clone(),
            Promise::spawn_async(
                async move { db::load_table_details(&pool, &schema, &table).await },
            ),
        ));
    }

    /// The pool queries should currently run against: the per-database pool
    /// of the last-selected table in browse-all mode, else the single pool.
    pub fn query_pool(&self) -> Option<PgPool> {
        if self.browse_all_active() {
            let db_name = self.query.active_database.as_ref()?;
            self.query.database_pools.get(db_name).cloned()
        } else {
            self.connection.active_pool.clone()
        }
    }

    /// `(total_connections, idle_connections)` for the pool queries would
    /// currently run against, if one exists. Synchronous — sqlx exposes
    /// pool size/idle counts without an await.
    pub fn pool_stats(&self) -> Option<(u32, usize)> {
        let pool = self.query_pool()?;
        Some((pool.size(), pool.num_idle()))
    }

    pub fn run_query(&mut self) {
        if self.query.sql.trim().is_empty() {
            return;
        }
        // Destructive statements need one explicit confirmation per edit;
        // the dialog clears the flag before re-running via confirm_run().
        if Self::is_destructive(&self.query.sql) && self.query.pending_confirm_sql.is_none() {
            self.query.pending_confirm_sql = Some(self.query.sql.clone());
            return;
        }
        self.execute_current_query();
    }

    /// "Run anyway" from the confirm dialog: bypasses the guard.
    pub fn confirm_run(&mut self) {
        self.query.pending_confirm_sql = None;
        self.execute_current_query();
    }

    /// Dispatch `EXPLAIN` for the current SQL. ANALYZE is refused for
    /// data-modifying statements up front: EXPLAIN ANALYZE genuinely executes
    /// them, so its effects would be written to the database.
    pub fn explain_query(&mut self, analyze: bool) {
        if self.query.sql.trim().is_empty() {
            return;
        }
        if analyze && Self::is_destructive(&self.query.sql) {
            self.query.status = "EXPLAIN ANALYZE is not available for data-modifying \
                                 statements \u{2014} ANALYZE would execute them for real."
                .into();
            return;
        }
        let Some(pool) = self.query_pool() else {
            if self.browse_all_active() {
                self.query.status = "Expand a database and pick a table first".into();
            }
            return;
        };
        self.query.explain_analyze = analyze;
        let sql = self.query.sql.clone();
        self.query.record_query_dispatch();
        self.query.status = if analyze {
            "Running EXPLAIN ANALYZE...".into()
        } else {
            "Running EXPLAIN...".into()
        };
        let handle = self.rt.spawn(async move {
            db::execute_explain(&pool, &sql, analyze)
                .await
                .map(|p| (p, analyze))
        });
        self.query.explain_cancel = Some(handle.abort_handle());
        self.query.explain_promise = Some(Promise::spawn_async(async move {
            match handle.await {
                Ok(res) => res,
                Err(_) => Err(anyhow::anyhow!("explain was cancelled")),
            }
        }));
    }

    /// Starts with a data-modifying keyword? (Plain prefix check on the
    /// trimmed, uppercased statement — CTEs/comments are not handled.)
    fn is_destructive(sql: &str) -> bool {
        let trimmed = sql.trim_start().to_uppercase();
        const KEYWORDS: &[&str] = &["DELETE", "UPDATE", "TRUNCATE", "DROP", "ALTER"];
        KEYWORDS.iter().any(|k| trimmed.starts_with(k))
    }

    fn execute_current_query(&mut self) {
        if self.query.sql.trim().is_empty() {
            return;
        }
        match self.query_pool() {
            Some(pool) => {
                let sql = self.query.sql.clone();
                self.query.record_query_dispatch();
                self.query.status = "Running query...".into();
                // A fresh query always starts at the first page.
                self.query.current_page = 0;
                self.query.has_next_page = false;
                let handle = self
                    .rt
                    .spawn(async move { db::execute_query(&pool, &sql).await });
                self.query.query_cancel = Some(handle.abort_handle());
                self.query.query_promise = Some(Promise::spawn_async(async move {
                    match handle.await {
                        Ok(res) => res,
                        Err(_) => Err(anyhow::anyhow!("query was cancelled")),
                    }
                }));
            }
            None => {
                if self.browse_all_active() {
                    self.query.status = "Expand a database and pick a table first".into();
                }
            }
        }
    }

    /// Fetch a specific page of results. Pages REPLACE the current result
    /// set — they never append to it.
    pub fn goto_page(&mut self, page: u64) {
        let Some(pool) = self.query_pool() else {
            return;
        };
        let limit: u64 = self.query.limit_value.parse().unwrap_or(100);
        let offset = page * limit;
        let sql = self.paginate_sql(limit, offset);
        self.query.record_query_dispatch();
        self.query.status = format!("Loading page {}...", page + 1);
        self.query.current_page = page;
        let handle = self
            .rt
            .spawn(async move { db::execute_query(&pool, &sql).await });
        self.query.query_cancel = Some(handle.abort_handle());
        self.query.query_promise = Some(Promise::spawn_async(async move {
            match handle.await {
                Ok(res) => res,
                Err(_) => Err(anyhow::anyhow!("query was cancelled")),
            }
        }));
    }

    pub fn next_page(&mut self) {
        if self.query.has_next_page {
            self.goto_page(self.query.current_page + 1);
        }
    }

    pub fn prev_page(&mut self) {
        if self.query.current_page > 0 {
            self.goto_page(self.query.current_page - 1);
        }
    }

    /// Open the cell-edit dialog for a result cell. `identity` is the
    /// `(primary-key column, value)` set that unambiguously locates the row;
    /// the cell's current value seeds the value box.
    #[allow(clippy::too_many_arguments)]
    pub fn open_edit_cell(
        &mut self,
        row_idx: usize,
        col_idx: usize,
        schema: &str,
        table: &str,
        column: &str,
        column_type: &str,
        nullable: bool,
        identity: Vec<(String, String)>,
    ) {
        let current = self
            .query
            .result
            .as_ref()
            .and_then(|r| r.rows.get(row_idx))
            .and_then(|row| row.get(col_idx))
            .cloned()
            .unwrap_or_default();
        self.query.edit = EditDialog {
            open: true,
            row_idx,
            col_idx,
            schema: schema.to_owned(),
            table: table.to_owned(),
            column: column.to_owned(),
            column_type: column_type.to_owned(),
            nullable,
            identity,
            value: current,
            saving: false,
            error: None,
            detail_expanded: false,
            promise: None,
        };
    }

    pub fn close_edit_cell(&mut self) {
        self.query.edit.promise = None; // drop the pending update, if any
        self.query.edit.open = false;
        self.query.edit.saving = false;
        self.query.edit.error = None;
    }

    /// Dispatch the UPDATE for the dialog's current value and remember the
    /// task; the result is applied by [`Self::poll_edit`].
    pub fn save_edit_cell(&mut self) {
        let e = &self.query.edit;
        if e.saving {
            return;
        }
        let Some(pool) = self.query_pool() else {
            self.query.edit.error = Some(crate::db::QueryError {
                headline: "No active connection".into(),
                detail: "The connection pool is gone; reconnect and try again.".into(),
            });
            return;
        };
        let value = e.value.clone();
        let schema = e.schema.clone();
        let table = e.table.clone();
        let column = e.column.clone();
        let identity = e.identity.clone();

        self.query.edit.saving = true;
        self.query.edit.error = None;
        self.query.edit.promise = Some(Promise::spawn_async(async move {
            crate::db::execute_cell_update(&pool, &schema, &table, &column, &value, &identity).await
        }));
    }

    /// Poll the in-flight cell edit. On success, closes the dialog and
    /// refreshes the current page so the grid shows the new value; on failure
    /// the structured error lands in the dialog's error slot. Returns true
    /// when the UI should repaint.
    pub fn poll_edit(&mut self) -> bool {
        if !self.query.edit.open {
            return false;
        }
        let Some(promise) = self.query.edit.promise.take() else {
            return false;
        };
        match promise.ready() {
            None => {
                self.query.edit.promise = Some(promise);
                false
            }
            Some(Ok(rows)) => {
                if *rows == 1 {
                    let note = format!(
                        "Updated {}.{}.{}",
                        self.query.edit.schema, self.query.edit.table, self.query.edit.column
                    );
                    self.query.edit.open = false;
                    self.query.edit.saving = false;
                    self.query.copy_feedback = Some(CopyFeedback {
                        text: note,
                        since: std::time::Instant::now(),
                    });
                    // Reload the page so the grid reflects the change.
                    let page = self.query.current_page;
                    self.goto_page(page);
                    true
                } else {
                    self.query.edit.saving = false;
                    self.query.edit.error = Some(crate::db::QueryError {
                        headline: format!("{rows} rows changed \u{2014} expected exactly 1"),
                        detail: "The row may have been modified or deleted after it was \
                                 loaded. Reload the results and try again."
                            .into(),
                    });
                    true
                }
            }
            Some(Err(e)) => {
                self.query.edit.saving = false;
                self.query.edit.error = Some(crate::db::QueryError::from_anyhow(e));
                true
            }
        }
    }

    /// Open the full-row edit dialog for a result row. Every non-PK result
    /// column that maps onto the selected table becomes an editable field
    /// (seeded with the row's current value); PK columns are captured as the
    /// read-only row identity used for the WHERE clause.
    pub fn open_edit_row(&mut self, row_idx: usize) {
        let Some(result) = self.query.result.as_ref() else {
            return;
        };
        let Some(table) = self.selected_table() else {
            return;
        };
        let Some(row) = result.rows.get(row_idx) else {
            return;
        };

        let mut identity: Vec<(String, String)> = Vec::new();
        let mut fields: Vec<RowEditField> = Vec::new();

        for (ci, name) in result.columns.iter().enumerate() {
            let Some(col) = table.columns.iter().find(|c| &c.name == name) else {
                // Not a plain projection of the selected table — abort
                // instead of producing a half-valid dialog.
                return;
            };
            let value = row.get(ci).cloned().unwrap_or_default();
            if col.is_pk {
                identity.push((name.clone(), value));
            } else {
                fields.push(RowEditField {
                    column: name.clone(),
                    column_type: result.column_types.get(ci).cloned().unwrap_or_default(),
                    nullable: col.nullable,
                    original_value: value.clone(),
                    value,
                    dirty: false,
                });
            }
        }

        if identity.is_empty() {
            return; // no usable PK — cannot build a safe WHERE
        }

        self.query.row_edit = RowEditDialog {
            open: true,
            row_idx,
            schema: table.schema.clone(),
            table: table.name.clone(),
            identity,
            fields,
            saving: false,
            error: None,
            detail_expanded: false,
            promise: None,
        };
    }

    pub fn close_edit_row(&mut self) {
        self.query.row_edit.promise = None; // drop the pending update, if any
        self.query.row_edit.open = false;
        self.query.row_edit.saving = false;
        self.query.row_edit.error = None;
    }

    /// Dispatch the dirty-only UPDATE for the dialog's current fields and
    /// remember the task; the result is applied by [`Self::poll_edit_row`].
    /// With nothing changed it just closes (a no-op save).
    pub fn save_edit_row(&mut self) {
        let d = &self.query.row_edit;
        if d.saving || !d.open {
            return;
        }
        let updates: Vec<(String, String)> = d
            .fields
            .iter()
            .filter(|f| f.dirty)
            .map(|f| (f.column.clone(), f.value.clone()))
            .collect();
        if updates.is_empty() {
            self.close_edit_row();
            return;
        }
        let Some(pool) = self.query_pool() else {
            self.query.row_edit.error = Some(crate::db::QueryError {
                headline: "No active connection".into(),
                detail: "The connection pool is gone; reconnect and try again.".into(),
            });
            return;
        };
        let schema = d.schema.clone();
        let table = d.table.clone();
        let identity = d.identity.clone();

        self.query.row_edit.saving = true;
        self.query.row_edit.error = None;
        self.query.row_edit.promise = Some(Promise::spawn_async(async move {
            crate::db::execute_row_update(&pool, &schema, &table, &updates, &identity).await
        }));
    }

    /// Poll the in-flight row edit. On success, closes the dialog and
    /// refreshes the current page so the grid shows the new values; on
    /// failure the structured error lands in the dialog's error slot. Returns
    /// true when the UI should repaint.
    pub fn poll_edit_row(&mut self) -> bool {
        if !self.query.row_edit.open {
            return false;
        }
        let Some(promise) = self.query.row_edit.promise.take() else {
            return false;
        };
        match promise.ready() {
            None => {
                self.query.row_edit.promise = Some(promise);
                false
            }
            Some(Ok(rows)) => {
                if *rows == 1 {
                    let note = format!(
                        "Updated {}.{}",
                        self.query.row_edit.schema, self.query.row_edit.table
                    );
                    self.query.row_edit.open = false;
                    self.query.row_edit.saving = false;
                    self.query.copy_feedback = Some(CopyFeedback {
                        text: note,
                        since: std::time::Instant::now(),
                    });
                    // Reload the page so the grid reflects the change.
                    let page = self.query.current_page;
                    self.goto_page(page);
                    true
                } else {
                    self.query.row_edit.saving = false;
                    self.query.row_edit.error = Some(crate::db::QueryError {
                        headline: format!("{rows} rows changed \u{2014} expected exactly 1"),
                        detail: "The row may have been modified or deleted after it was \
                                 loaded. Reload the results and try again."
                            .into(),
                    });
                    true
                }
            }
            Some(Err(e)) => {
                self.query.row_edit.saving = false;
                self.query.row_edit.error = Some(crate::db::QueryError::from_anyhow(e));
                true
            }
        }
    }

    pub fn visible_schemas_len(&self) -> usize {
        if self.browse_all_active() {
            self.query
                .active_database
                .as_ref()
                .and_then(|db| self.query.databases.iter().find(|d| &d.name == db))
                .map(|d| d.schemas.len())
                .unwrap_or(0)
        } else {
            self.query.schemas.len()
        }
    }

    pub fn selected_schema(&self) -> Option<&db::SchemaInfo> {
        if self.browse_all_active() {
            let db_name = self.query.active_database.as_ref()?;
            self.query
                .databases
                .iter()
                .find(|d| &d.name == db_name)
                .and_then(|d| d.schemas.get(self.query.schema_idx))
        } else {
            self.query.schemas.get(self.query.schema_idx)
        }
    }

    pub fn selected_table(&self) -> Option<&db::TableInfo> {
        self.selected_schema()
            .and_then(|s| s.tables.get(self.query.table_idx))
    }

    /// Flat identifier list for autocomplete: schema, table and column names
    /// from whatever tree is currently loaded. Tagged by kind, deduped,
    /// sorted case-insensitively (relevance ranking happens at filter time).
    pub fn autocomplete_candidates(&self) -> Vec<Candidate> {
        let schemas: &[db::SchemaInfo] = if self.browse_all_active() {
            match self.query.active_database.as_deref() {
                Some(name) => self
                    .query
                    .databases
                    .iter()
                    .find(|d| d.name == name)
                    .map(|d| d.schemas.as_slice())
                    .unwrap_or(&[]),
                None => &[],
            }
        } else {
            &self.query.schemas
        };

        let mut out: Vec<Candidate> = Vec::new();
        for s in schemas {
            out.push(Candidate {
                name: s.name.clone(),
                kind: CandidateKind::Schema,
            });
            for t in &s.tables {
                out.push(Candidate {
                    name: t.name.clone(),
                    kind: CandidateKind::Table,
                });
                for c in &t.columns {
                    out.push(Candidate {
                        name: c.name.clone(),
                        kind: CandidateKind::Column,
                    });
                }
            }
        }
        out.sort_by_key(|a| a.name.to_lowercase());
        out.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name) && a.kind == b.kind);
        out
    }

    /// Splice a chosen suggestion over the word under the caret. Returns the
    /// new caret position in chars (egui's `CCursor` is char-indexed), or
    /// `None` when nothing was spliced.
    pub fn autocomplete_insert(&mut self, choice: &str) -> Option<usize> {
        let range = self.query.autocomplete_splice.take()?;
        if range.start > range.end || range.end > self.query.sql.len() {
            self.query.autocomplete_matches.clear();
            self.query.autocomplete_open = false;
            return None;
        }
        self.query.sql.replace_range(range.clone(), choice);
        self.query.autocomplete_matches.clear();
        self.query.autocomplete_open = false;

        // char count up to the end of the just-inserted text
        let end_byte = range.start + choice.len();
        let new_char_idx = self.query.sql[..end_byte].chars().count();
        Some(new_char_idx)
    }

    /// Dismiss the autocomplete popup and forget its suggestions.
    pub fn close_autocomplete(&mut self) {
        self.query.autocomplete_open = false;
        self.query.autocomplete_matches.clear();
        self.query.autocomplete_selected = 0;
        self.query.autocomplete_splice = None;
    }

    /// Move the popup's highlighted row by `delta`, clamped to the list.
    pub fn move_autocomplete_selection(&mut self, delta: isize) {
        let len = self.query.autocomplete_matches.len();
        if len == 0 {
            return;
        }
        let next = self.query.autocomplete_selected as isize + delta;
        self.query.autocomplete_selected = next.clamp(0, len as isize - 1) as usize;
    }

    /// Insert the currently highlighted suggestion into the query. Returns
    /// the new caret position in chars, if a suggestion was inserted.
    pub fn accept_autocomplete(&mut self) -> Option<usize> {
        let choice = self
            .query
            .autocomplete_matches
            .get(self.query.autocomplete_selected)
            .cloned()?;
        self.autocomplete_insert(&choice.name)
    }

    pub fn select_table(&mut self, si: usize, ti: usize) {
        if si < self.query.schemas.len() && ti < self.query.schemas[si].tables.len() {
            self.query.schema_idx = si;
            self.query.table_idx = ti;
            // Switching tables must not carry over a stale page number.
            self.query.current_page = 0;
            self.query.has_next_page = false;
            self.update_sql();
        }
    }

    /// Select a table under a specific database (browse-all mode).
    pub fn select_table_in_database(&mut self, db_name: &str, si: usize, ti: usize) {
        if let Some(info) = self.query.databases.iter().find(|d| d.name == db_name)
            && si < info.schemas.len()
            && ti < info.schemas[si].tables.len()
        {
            self.query.active_database = Some(db_name.to_string());
            self.query.schema_idx = si;
            self.query.table_idx = ti;
            // Switching tables must not carry over a stale page number.
            self.query.current_page = 0;
            self.query.has_next_page = false;
            self.update_sql();
        }
    }

    pub fn update_sql(&mut self) {
        if let Some(table) = self.selected_table() {
            let limit = self.query.limit_value.parse::<u64>().unwrap_or(1000);
            let offset = self.query.offset_value.parse::<u64>().unwrap_or(0);
            let mut qb = QueryBuilder::new(&table.schema, &table.name);
            qb.set_limit(limit).set_offset(offset);
            self.query.sql = qb.to_sql();
        } else {
            self.query.sql.clear();
        }
    }

    fn paginate_sql(&self, new_limit: u64, new_offset: u64) -> String {
        let trimmed = self.query.sql.trim();
        let upper = trimmed.to_uppercase();
        if let Some(pos) = upper.rfind("LIMIT") {
            let before = &trimmed[..pos].trim_end();
            format!("{} LIMIT {} OFFSET {}", before, new_limit, new_offset)
        } else {
            format!("{} LIMIT {} OFFSET {}", trimmed, new_limit, new_offset)
        }
    }
}

#[cfg(test)]
mod query_tracking_tests {
    use super::*;

    /// The three record_query_* methods live on QueryState (not AppState)
    /// precisely so they're testable without a tokio runtime or PgPool.
    #[test]
    fn dispatch_increments_count_and_clears_previous_error() {
        let mut q = QueryState {
            last_error: Some(crate::db::QueryError {
                headline: "old".into(),
                detail: "old".into(),
            }),
            error_detail_expanded: true,
            ..QueryState::default()
        };

        q.record_query_dispatch();

        assert_eq!(q.session_query_count, 1);
        assert!(
            q.last_error.is_none(),
            "a new dispatch clears the old error"
        );
        assert!(!q.error_detail_expanded);
        assert!(q.last_run_started_at.is_some());
    }

    #[test]
    fn failure_increments_failed_count_and_sets_error() {
        let mut q = QueryState::default();
        q.record_query_dispatch();
        let err = anyhow::anyhow!("syntax error at or near \"on\"");

        q.record_query_failure(&err);

        assert_eq!(q.session_failed_count, 1);
        assert_eq!(
            q.last_error.as_ref().map(|e| e.headline.as_str()),
            Some("syntax error at or near \"on\"")
        );
        assert!(
            q.last_run_duration.is_some(),
            "failure also records duration"
        );
        assert!(q.last_run_started_at.is_none(), "started_at is consumed");
    }

    #[test]
    fn success_records_duration_and_consumes_started_at() {
        let mut q = QueryState::default();
        q.record_query_dispatch();

        q.record_query_success();

        assert_eq!(
            q.session_failed_count, 0,
            "success must not count as failure"
        );
        assert!(q.last_run_duration.is_some());
        assert!(q.last_run_started_at.is_none());
    }

    #[test]
    fn success_clears_previous_error() {
        // A query that succeeds after a failure closes the error panel.
        let mut q = QueryState::default();
        q.record_query_dispatch();
        q.record_query_failure(&anyhow::anyhow!("boom"));
        assert!(q.last_error.is_some());

        q.record_query_dispatch();
        q.record_query_success();

        assert!(q.last_error.is_none(), "success path leaves no error panel");
    }

    #[test]
    fn success_without_dispatch_is_a_no_op() {
        // Poll safety: record_query_success/failure may only fire once per
        // promise, but a second call (or one with no dispatch) must not
        // panic or corrupt counters.
        let mut q = QueryState::default();
        q.record_query_success();
        q.record_query_failure(&anyhow::anyhow!("late"));
        assert_eq!(q.session_failed_count, 1);
        assert!(q.last_run_duration.is_none());
    }
}
