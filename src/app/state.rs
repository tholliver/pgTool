use crate::connections::{ConnectionProfile, SslMode};
use crate::db;
use crate::query_builder::QueryBuilder;
use poll_promise::Promise;
use sqlx::PgPool;

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

pub struct ConnectionState {
    pub profiles: Vec<ConnectionProfile>,
    pub active_id: Option<String>,
    pub active_pool: Option<PgPool>,
    pub dialog: ConnectionDialog,
    pub menu_open: bool,
}

pub struct ConnectionDialog {
    pub open: bool,
    pub editing_id: Option<String>,
    pub name: String,
    pub host: String,
    pub port: String,
    pub database: String,
    /// When true, the Database field is ignored and the connection lists
    /// every database on the server.
    pub browse_all_databases: bool,
    pub username: String,
    pub password: String,
    pub ssl_mode: SslMode,
    pub error: Option<String>,
    pub testing: bool,
    pub test_promise: Option<Promise<Result<PgPool, anyhow::Error>>>,
}

impl Default for ConnectionDialog {
    fn default() -> Self {
        Self {
            open: false,
            editing_id: None,
            name: String::new(),
            host: "localhost".into(),
            port: "5432".into(),
            database: String::new(),
            browse_all_databases: false,
            username: "postgres".into(),
            password: String::new(),
            ssl_mode: SslMode::Disable,
            error: None,
            testing: false,
            test_promise: None,
        }
    }
}

impl ConnectionDialog {
    pub fn open_add(&mut self) {
        self.open = true;
        self.editing_id = None;
        self.name.clear();
        self.host = "localhost".into();
        self.port = "5432".into();
        self.database.clear();
        self.browse_all_databases = false;
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
        self.name = profile.name.clone();
        self.host = profile.host.clone();
        self.port = profile.port.to_string();
        self.database = profile.database.clone().unwrap_or_default();
        self.browse_all_databases = profile.browse_all_databases();
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
    }

    pub fn build_profile(&self) -> ConnectionProfile {
        let port = self.port.parse::<u16>().unwrap_or(5432);
        let database = if self.browse_all_databases {
            None
        } else {
            Some(self.database.clone())
        };
        let id = self.editing_id.clone().unwrap_or_else(|| {
            ConnectionProfile::credential_id(&self.host, port, database.as_deref(), &self.ssl_mode)
        });
        let name = if self.name.trim().is_empty() {
            format!("{}:{}", self.host, port)
        } else {
            self.name.clone()
        };
        ConnectionProfile {
            id,
            name,
            host: self.host.clone(),
            port,
            database,
            username: self.username.clone(),
            ssl_mode: self.ssl_mode.clone(),
            password: self.password.clone(),
        }
    }

    pub fn connection_url(&self) -> String {
        let profile = self.build_profile();
        profile.connection_url(&self.password)
    }

    pub fn start_test(&mut self) {
        self.error = None;
        self.testing = true;
        let url = self.connection_url();
        self.test_promise = Some(Promise::spawn_async(async move {
            crate::db::connect_with_url(&url).await
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
    /// Results grid selection `(row, column)` (0-based, into the rows as
    /// displayed this frame). Currently drives copy; reserved as the anchor
    /// for cell editing in a later release.
    pub selected_cell: Option<(usize, usize)>,
    /// Transient "copied cell" feedback shown in the results panel.
    pub copy_feedback: Option<CopyFeedback>,
    /// Modal cell-edit dialog (opened from the results grid context menu).
    pub edit: EditDialog,
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
            selected_cell: None,
            copy_feedback: None,
            edit: EditDialog::default(),
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
        let profiles = crate::connections::load_profiles().unwrap_or_default();
        Self {
            connection: ConnectionState {
                profiles,
                active_id: None,
                active_pool: None,
                dialog: ConnectionDialog::default(),
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

        let url = profile.connection_url_for_database(&profile.password, db_name);
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
            let url = profile.connection_url(&profile.password);
            let profile_id = profile.id.clone();
            // We need to connect async — spawn it
            let pool_promise =
                poll_promise::Promise::spawn_async(async move { db::connect_with_url(&url).await });
            // Store the promise and profile_id for polling
            self.query.status = format!("Connecting to {}...", profile.name);
            self.connection.active_id = Some(profile_id.clone());
            self.query.expanded_connections.insert(profile_id);
            self.clear_query_state();
            // We'll need to handle this connect promise in poll_promises
            // For now, store it as a special field
            self.connect_promise = Some(pool_promise);
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
                    self.connect_promise = None;
                    changed = true;
                }
            }
        }

        // Poll dialog test promise
        if let Some(p) = &self.connection.dialog.test_promise {
            match p.ready() {
                None => {}
                Some(Ok(pool)) => {
                    let profile = self.connection.dialog.build_profile();
                    // Upsert profile: replace if same ID exists, otherwise push
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
                    let _ = crate::connections::save_profiles(&self.connection.profiles);
                    // Connect
                    self.query.expanded_connections.insert(profile.id.clone());
                    self.connection.active_id = Some(profile.id);
                    self.connection.active_pool = Some(pool.clone());
                    self.connection.dialog.close();
                    self.clear_query_state();
                    self.start_post_connect_load();
                    self.query.status = "Connected".into();
                    self.connection.dialog.test_promise = None;
                    changed = true;
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
                    changed = true;
                }
                Some(Err(e)) => {
                    self.query.status = format!("Schema load error: {e}");
                    self.query.schema_promise = None;
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

        changed
    }

    pub fn is_loading(&self) -> bool {
        self.connect_promise.is_some()
            || self.query.schema_promise.is_some()
            || self.query.database_promise.is_some()
            || self.query.db_schema_promise.is_some()
            || self.query.query_promise.is_some()
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
                // Snap the editor/results split back to 50/50 on every fresh run.
                self.query.results_split_ratio = 0.5;
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
                self.query.edit.error = Some(crate::db::QueryError::from_anyhow(&e));
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
