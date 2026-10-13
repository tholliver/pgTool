#[derive(Debug, Clone)]
pub struct SchemaInfo {
    pub name: String,
    pub tables: Vec<TableInfo>,
    pub views: Vec<ViewInfo>,
    pub sequences: Vec<SequenceInfo>,
    pub functions: Vec<FunctionInfo>,
}

/// A database on the connected server. Schemas are lazy-loaded on expand.
#[derive(Debug, Clone)]
pub struct DatabaseInfo {
    pub name: String,
    pub schemas: Vec<SchemaInfo>,
    pub loaded: bool,
    pub extensions: Vec<ExtensionInfo>,
}

#[derive(Debug, Clone)]
pub struct TableInfo {
    pub schema: String,
    pub name: String,
    pub columns: Vec<ColumnInfo>,
    /// Planner's row estimate from pg_class.reltuples; None when unknown
    /// (table never analyzed). An estimate, not an exact count.
    pub row_count: Option<i64>,
    /// True once indexes/fks/triggers have been fetched for this table
    /// (lazy, on table-node expand).
    pub detail_loaded: bool,
    pub indexes: Vec<IndexInfo>,
    pub foreign_keys: Vec<ForeignKeyInfo>,
    pub triggers: Vec<TriggerInfo>,
}

#[derive(Debug, Clone)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub is_pk: bool,
}

/// A regular or materialized view in a schema.
#[derive(Debug, Clone)]
pub struct ViewInfo {
    pub schema: String,
    pub name: String,
    pub columns: Vec<ColumnInfo>,
    pub is_materialized: bool,
    pub definition: String,
}

/// One index on a table. `definition` is the ready-made `indexdef` DDL string
/// from `pg_indexes`; `columns` is the best-effort parsed column list.
#[derive(Debug, Clone)]
pub struct IndexInfo {
    pub name: String,
    pub table: String,
    pub columns: Vec<String>,
    pub is_unique: bool,
    pub is_primary: bool,
    pub definition: String,
}

/// A foreign key declared on a table (the referencing side). `ref_table` is
/// stored schema-qualified (`schema.table`) so it is unambiguous in browse
/// mode.
#[derive(Debug, Clone)]
pub struct ForeignKeyInfo {
    pub name: String,
    pub table: String,
    pub columns: Vec<String>,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
}

/// A sequence in a schema. `owned_by` is the `schema.table.column` it backs
/// (if any), or None for stand-alone sequences.
#[derive(Debug, Clone)]
pub struct SequenceInfo {
    pub schema: String,
    pub name: String,
    pub data_type: String,
    pub owned_by: Option<String>,
}

/// A function or procedure in a schema. `arguments`/`return_type` carry the
/// formatted signatures; `kind` is "function" or "procedure".
#[derive(Debug, Clone)]
pub struct FunctionInfo {
    pub schema: String,
    pub name: String,
    pub arguments: String,
    pub return_type: String,
    pub kind: String,
}

/// A trigger on a table.
#[derive(Debug, Clone)]
pub struct TriggerInfo {
    pub name: String,
    pub table: String,
    pub event: String,
    pub timing: String,
    pub function: String,
}

/// An installed extension in the connected database.
#[derive(Debug, Clone)]
pub struct ExtensionInfo {
    pub name: String,
    pub version: String,
    pub schema: String,
}

/// The lazily-fetched per-table detail set (indexes, foreign keys, triggers),
/// loaded together when a table node is first expanded.
#[derive(Debug, Clone, Default)]
pub struct TableDetails {
    pub indexes: Vec<IndexInfo>,
    pub foreign_keys: Vec<ForeignKeyInfo>,
    pub triggers: Vec<TriggerInfo>,
}

/// Result of an EXPLAIN run: the JSON-encoded plan tree plus how it was run
/// and how long the analysis (if any) took.
#[derive(Debug, Clone)]
pub struct ExplainResult {
    /// The plan as a JSON array string, exactly as Postgres returned it.
    pub plan: String,
    /// Whether this run used `ANALYZE` (i.e. the query actually executed).
    pub analyze: bool,
    /// Wall-clock time of the round trip, in milliseconds.
    pub elapsed_ms: u64,
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
