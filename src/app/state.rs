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
    /// Fraction of the shared editor/results split given to the SQL editor (0.0–1.0).
    pub results_split_ratio: f32,
    pub expanded_schemas: std::collections::HashSet<usize>,
    /// Schema expansion scoped per database in browse-all mode: (db, schema_idx).
    pub expanded_db_schemas: std::collections::HashSet<(String, usize)>,
    pub expanded_connections: std::collections::HashSet<String>,
}

pub struct AppState {
    pub connection: ConnectionState,
    pub query: QueryState,
    pub connect_promise: Option<Promise<Result<PgPool, anyhow::Error>>>,
}

impl AppState {
    pub fn new() -> Self {
        let profiles = crate::connections::load_profiles().unwrap_or_default();
        Self {
            connection: ConnectionState {
                profiles,
                active_id: None,
                active_pool: None,
                dialog: ConnectionDialog::default(),
                menu_open: false,
            },
            query: QueryState {
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
                results_split_ratio: 0.5,
                expanded_schemas: std::collections::HashSet::new(),
                expanded_db_schemas: std::collections::HashSet::new(),
                expanded_connections: std::collections::HashSet::new(),
            },
            connect_promise: None,
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
        if self.query.database_pools.contains_key(db_name) {
            return;
        }
        // Only one lazy connect at a time — don't clobber an in-flight one.
        if let Some((pending_name, promise)) = &self.query.db_schema_promise
            && (pending_name == db_name || promise.ready().is_none())
        {
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
        self.query.schema_promise = None;
        self.query.database_promise = None;
        self.query.db_schema_promise = None;
        self.query.query_promise = None;
        self.query.current_page = 0;
        self.query.has_next_page = false;
        self.query.filter_text.clear();
        self.query.expanded_schemas.clear();
        self.query.expanded_db_schemas.clear();
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
            if let Some(info) = self
                .query
                .databases
                .iter_mut()
                .find(|d| d.name == name)
            {
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

        // Poll query promise
        if let Some(p) = &self.query.query_promise {
            match p.ready() {
                None => {}
                Some(Ok(result)) => {
                    let rows = result.rows.len();
                    // A full page means there are probably more rows after it.
                    let limit: u64 = self.query.limit_value.parse().unwrap_or(100);
                    self.query.has_next_page = rows as u64 == limit;
                    self.query.result = Some(result.clone());
                    self.query.status = format!(
                        "{rows} rows returned \u{2014} page {}",
                        self.query.current_page + 1
                    );
                    self.query.query_promise = None;
                    changed = true;
                }
                Some(Err(e)) => {
                    self.query.status = format!("Query error: {e}");
                    self.query.query_promise = None;
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

    pub fn run_query(&mut self) {
        if self.query.sql.trim().is_empty() {
            return;
        }
        match self.query_pool() {
            Some(pool) => {
                let sql = self.query.sql.clone();
                self.query.status = "Running query...".into();
                // A fresh query always starts at the first page.
                self.query.current_page = 0;
                self.query.has_next_page = false;
                // Snap the editor/results split back to 50/50 on every fresh run.
                self.query.results_split_ratio = 0.5;
                self.query.query_promise = Some(Promise::spawn_async(async move {
                    db::execute_query(&pool, &sql).await
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
        self.query.status = format!("Loading page {}...", page + 1);
        self.query.current_page = page;
        self.query.query_promise = Some(Promise::spawn_async(async move {
            db::execute_query(&pool, &sql).await
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
