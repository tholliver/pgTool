#[derive(Debug, Clone)]
pub struct SchemaInfo {
    pub name: String,
    pub tables: Vec<TableInfo>,
}

/// A database on the connected server. Schemas are lazy-loaded on expand.
#[derive(Debug, Clone)]
pub struct DatabaseInfo {
    pub name: String,
    pub schemas: Vec<SchemaInfo>,
    pub loaded: bool,
}

#[derive(Debug, Clone)]
pub struct TableInfo {
    pub schema: String,
    pub name: String,
    pub columns: Vec<ColumnInfo>,
    /// Planner's row estimate from pg_class.reltuples; None when unknown
    /// (table never analyzed). An estimate, not an exact count.
    pub row_count: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub is_pk: bool,
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub columns: Vec<String>,
    /// Column type names (PostgreSQL `PgTypeInfo::name()`, e.g. "INT4",
    /// "TEXT", "TIMESTAMPTZ"). Parallel to `columns`.
    pub column_types: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub rows_affected: u64,
}
