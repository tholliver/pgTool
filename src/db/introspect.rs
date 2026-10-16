use anyhow::Result;
use sqlx::{PgPool, Row};

use super::models::{
    ColumnInfo, DatabaseInfo, ExtensionInfo, ForeignKeyInfo, FunctionInfo, IndexInfo, SchemaInfo,
    SequenceInfo, TableDetails, TableInfo, TriggerInfo, ViewInfo,
};

/// Every non-template database on the server that the current user may
/// connect to. Schemas are lazy-loaded per database via `load_schemas`.
pub async fn list_databases(pool: &PgPool) -> Result<Vec<DatabaseInfo>> {
    let rows = sqlx::query(
        "SELECT datname FROM pg_database
         WHERE datistemplate = false
           AND has_database_privilege(datname, 'CONNECT')
         ORDER BY datname",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| DatabaseInfo {
            name: r.get("datname"),
            schemas: Vec::new(),
            loaded: false,
            extensions: Vec::new(),
        })
        .collect())
}

pub async fn load_schemas(pool: &PgPool) -> Result<Vec<SchemaInfo>> {
    let schema_rows = sqlx::query(
        "SELECT schema_name FROM information_schema.schemata
         WHERE schema_name NOT IN ('pg_catalog','information_schema','pg_toast')
         ORDER BY schema_name",
    )
    .fetch_all(pool)
    .await?;

    let mut schemas = Vec::new();

    for sr in &schema_rows {
        let schema_name: String = sr.get("schema_name");
        let tables = load_tables(pool, &schema_name).await?;
        let views = load_views(pool, &schema_name).await?;
        let sequences = load_sequences(pool, &schema_name).await?;
        let functions = load_functions(pool, &schema_name).await?;
        schemas.push(SchemaInfo {
            name: schema_name,
            tables,
            views,
            sequences,
            functions,
        });
    }

    Ok(schemas)
}

/// Database-level objects for the connected database: installed extensions.
/// Called once per connection (not per schema).
pub async fn load_extensions(pool: &PgPool) -> Result<Vec<ExtensionInfo>> {
    let rows = sqlx::query(
        "SELECT e.extname AS name,
                e.extversion AS version,
                n.nspname AS schema
         FROM pg_extension e
         LEFT JOIN pg_namespace n ON n.oid = e.extnamespace
         ORDER BY e.extname",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| ExtensionInfo {
            name: r.get("name"),
            version: r.get("version"),
            schema: r.get::<Option<String>, _>("schema").unwrap_or_default(),
        })
        .collect())
}

async fn load_tables(pool: &PgPool, schema: &str) -> Result<Vec<TableInfo>> {
    // reltuples is the planner's estimate: -1 means "never analyzed"
    // (unknown), >= 0 is an approximate row count. Joining pg_class here
    // avoids one COUNT(*) round trip per table.
    let table_rows = sqlx::query(
        "SELECT t.table_name,
                CASE WHEN c.reltuples >= 0 THEN c.reltuples::bigint END AS est_rows
         FROM information_schema.tables t
         LEFT JOIN pg_namespace n ON n.nspname = t.table_schema
         LEFT JOIN pg_class c
              ON c.relnamespace = n.oid AND c.relname = t.table_name
             AND c.relkind IN ('r','p')
         WHERE t.table_schema = $1 AND t.table_type = 'BASE TABLE'
         ORDER BY t.table_name",
    )
    .bind(schema)
    .fetch_all(pool)
    .await?;

    let mut tables = Vec::new();

    for tr in &table_rows {
        let table_name: String = tr.get("table_name");
        let columns = load_columns(pool, schema, &table_name).await?;
        tables.push(TableInfo {
            schema: schema.to_string(),
            name: table_name,
            columns,
            row_count: tr.get("est_rows"),
            detail_loaded: false,
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            triggers: Vec::new(),
        });
    }

    Ok(tables)
}

async fn load_columns(pool: &PgPool, schema: &str, table: &str) -> Result<Vec<ColumnInfo>> {
    let rows = sqlx::query(
        "SELECT
            c.column_name,
            c.data_type,
            c.is_nullable,
            CASE WHEN pk.column_name IS NOT NULL THEN true ELSE false END AS is_pk
         FROM information_schema.columns c
         LEFT JOIN (
             SELECT ku.column_name
             FROM information_schema.table_constraints tc
             JOIN information_schema.key_column_usage ku
               ON tc.constraint_name = ku.constraint_name
              AND tc.table_schema    = ku.table_schema
             WHERE tc.constraint_type = 'PRIMARY KEY'
               AND tc.table_schema    = $1
               AND tc.table_name      = $2
         ) pk ON pk.column_name = c.column_name
         WHERE c.table_schema = $1 AND c.table_name = $2
         ORDER BY c.ordinal_position",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| ColumnInfo {
            name: r.get("column_name"),
            data_type: r.get("data_type"),
            nullable: r.get::<String, _>("is_nullable") == "YES",
            is_pk: r.get("is_pk"),
        })
        .collect())
}

/// Regular and materialized views in a schema, with their columns (loaded in
/// one extra query, not one per view) and their stored definition.
pub async fn load_views(pool: &PgPool, schema: &str) -> Result<Vec<ViewInfo>> {
    let regular: Vec<(String, String)> = sqlx::query(
        "SELECT table_name, view_definition
         FROM information_schema.views
         WHERE table_schema = $1
         ORDER BY table_name",
    )
    .bind(schema)
    .fetch_all(pool)
    .await?
    .iter()
    .map(|r| {
        (
            r.get::<String, _>("table_name"),
            r.get::<Option<String>, _>("view_definition")
                .unwrap_or_default(),
        )
    })
    .collect();

    let materialized: Vec<(String, String)> = sqlx::query(
        "SELECT matviewname, definition
         FROM pg_matviews
         WHERE schemaname = $1
         ORDER BY matviewname",
    )
    .bind(schema)
    .fetch_all(pool)
    .await?
    .iter()
    .map(|r| (r.get("matviewname"), r.get("definition")))
    .collect();

    load_views_inner(pool, schema, regular, materialized).await
}

/// Loads columns for every view name in one query and assembles `ViewInfo`s.
async fn load_views_inner(
    pool: &PgPool,
    schema: &str,
    regular: Vec<(String, String)>,
    materialized: Vec<(String, String)>,
) -> Result<Vec<ViewInfo>> {
    let mut entries: Vec<(String, String, bool)> = Vec::new();
    for (name, def) in regular {
        entries.push((name, def, false));
    }
    for (name, def) in materialized {
        entries.push((name, def, true));
    }

    let col_rows = sqlx::query(
        "SELECT table_name, column_name, data_type, is_nullable
         FROM information_schema.columns
         WHERE table_schema = $1
         ORDER BY table_name, ordinal_position",
    )
    .bind(schema)
    .fetch_all(pool)
    .await?;

    let mut by_view: std::collections::HashMap<String, Vec<ColumnInfo>> =
        std::collections::HashMap::new();
    for r in &col_rows {
        let table_name: String = r.get("table_name");
        if !entries.iter().any(|(n, _, _)| n == &table_name) {
            continue;
        }
        by_view.entry(table_name).or_default().push(ColumnInfo {
            name: r.get("column_name"),
            data_type: r.get("data_type"),
            nullable: r.get::<String, _>("is_nullable") == "YES",
            is_pk: false,
        });
    }

    Ok(entries
        .into_iter()
        .map(|(name, definition, is_materialized)| {
            let columns = by_view.remove(&name).unwrap_or_default();
            ViewInfo {
                schema: schema.to_string(),
                name,
                columns,
                is_materialized,
                definition,
            }
        })
        .collect())
}

/// Sequences in a schema, including which table column each one backs
/// (`pg_depend` "auto" dependency) when applicable.
pub async fn load_sequences(pool: &PgPool, schema: &str) -> Result<Vec<SequenceInfo>> {
    let rows = sqlx::query(
        "SELECT seq.relname AS name,
                format_type(s.seqtypid, NULL) AS data_type,
                CASE WHEN dep.refobjid IS NOT NULL
                     THEN tbl.nspname || '.' || dep_c.relname || '.' || a.attname
                     ELSE NULL END AS owned_by
         FROM pg_sequence s
         JOIN pg_class seq ON seq.oid = s.seqrelid
         JOIN pg_namespace n ON n.oid = seq.relnamespace
         LEFT JOIN pg_depend dep
                ON dep.classid = 'pg_class'::regclass
               AND dep.objid = seq.oid
               AND dep.refobjsubid > 0
         LEFT JOIN pg_class dep_c ON dep_c.oid = dep.refobjid
         LEFT JOIN pg_namespace tbl ON tbl.oid = dep_c.relnamespace
         LEFT JOIN pg_attribute a
                ON a.attrelid = dep.refobjid AND a.attnum = dep.refobjsubid
         WHERE n.nspname = $1
         ORDER BY seq.relname",
    )
    .bind(schema)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| SequenceInfo {
            schema: schema.to_string(),
            name: r.get("name"),
            data_type: r.get("data_type"),
            owned_by: r.get("owned_by"),
        })
        .collect())
}

/// Functions and procedures in a schema, with formatted signatures.
pub async fn load_functions(pool: &PgPool, schema: &str) -> Result<Vec<FunctionInfo>> {
    let rows = sqlx::query(
        "SELECT p.proname AS name,
                pg_get_function_arguments(p.oid) AS arguments,
                pg_get_function_result(p.oid) AS return_type,
                CASE p.prokind WHEN 'p' THEN 'procedure' ELSE 'function' END AS kind
         FROM pg_proc p
         JOIN pg_namespace n ON n.oid = p.pronamespace
         WHERE n.nspname = $1
           AND p.prokind IN ('f', 'p')
         ORDER BY p.proname",
    )
    .bind(schema)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| FunctionInfo {
            schema: schema.to_string(),
            name: r.get("name"),
            arguments: r.get("arguments"),
            return_type: r.get("return_type"),
            kind: r.get("kind"),
        })
        .collect())
}

/// Indexes on one table. `definition` is the ready-made `indexdef` string;
/// the parsed `columns` come from `pg_index.indkey`.
pub async fn load_indexes(pool: &PgPool, schema: &str, table: &str) -> Result<Vec<IndexInfo>> {
    let rows = sqlx::query(
        "SELECT i.relname AS name,
                ix.indisunique AS is_unique,
                ix.indisprimary AS is_primary,
                pg_get_indexdef(i.oid) AS definition,
                ARRAY(
                    SELECT a.attname
                    FROM unnest(ix.indkey) WITH ORDINALITY AS k(attnum, ord)
                    JOIN pg_attribute a
                      ON a.attrelid = i.indrelid AND a.attnum = k.attnum
                    ORDER BY k.ord
                ) AS columns
         FROM pg_index ix
         JOIN pg_class i ON i.oid = ix.indexrelid
         JOIN pg_class t ON t.oid = ix.indrelid
         JOIN pg_namespace n ON n.oid = t.relnamespace
         WHERE n.nspname = $1 AND t.relname = $2
         ORDER BY i.relname",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| IndexInfo {
            name: r.get("name"),
            table: table.to_string(),
            columns: r.get("columns"),
            is_unique: r.get("is_unique"),
            is_primary: r.get("is_primary"),
            definition: r.get("definition"),
        })
        .collect())
}

/// Foreign keys declared on one table, each grouped into its constraint.
/// `ref_table` is stored schema-qualified.
pub async fn load_foreign_keys(
    pool: &PgPool,
    schema: &str,
    table: &str,
) -> Result<Vec<ForeignKeyInfo>> {
    let rows = sqlx::query(
        "SELECT tc.constraint_name AS name,
                kcu.column_name AS column,
                ccu.table_schema AS ref_schema,
                ccu.table_name AS ref_table,
                ccu.column_name AS ref_column
         FROM information_schema.table_constraints tc
         JOIN information_schema.key_column_usage kcu
           ON tc.constraint_name = kcu.constraint_name
          AND tc.table_schema = kcu.table_schema
         JOIN information_schema.constraint_column_usage ccu
           ON tc.constraint_name = ccu.constraint_name
          AND tc.table_schema = ccu.table_schema
         WHERE tc.constraint_type = 'FOREIGN KEY'
           AND tc.table_schema = $1
           AND tc.table_name = $2
         ORDER BY tc.constraint_name, kcu.ordinal_position",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await?;

    let mut map: std::collections::HashMap<String, (Vec<String>, String, Vec<String>)> =
        std::collections::HashMap::new();
    for r in &rows {
        let name: String = r.get("name");
        let column: String = r.get("column");
        let ref_schema: String = r.get("ref_schema");
        let ref_table: String = r.get("ref_table");
        let ref_column: String = r.get("ref_column");
        let qualified_ref = if ref_schema == schema {
            ref_table.clone()
        } else {
            format!("{ref_schema}.{ref_table}")
        };
        let e = map
            .entry(name)
            .or_insert_with(|| (Vec::new(), String::new(), Vec::new()));
        e.0.push(column);
        if e.1.is_empty() {
            e.1 = qualified_ref;
        }
        e.2.push(ref_column);
    }

    let mut out: Vec<ForeignKeyInfo> = map
        .into_iter()
        .map(|(name, (columns, ref_table, ref_columns))| ForeignKeyInfo {
            name,
            table: table.to_string(),
            columns,
            ref_table,
            ref_columns,
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Triggers on one table plus indexes and foreign keys, fetched together on
/// demand when a table node is expanded (keeps the eager schema load cheap —
/// one query per table, not four).
pub async fn load_table_details(pool: &PgPool, schema: &str, table: &str) -> Result<TableDetails> {
    let indexes = load_indexes(pool, schema, table).await?;
    let foreign_keys = load_foreign_keys(pool, schema, table).await?;
    let triggers = load_triggers(pool, schema, table).await?;
    Ok(TableDetails {
        indexes,
        foreign_keys,
        triggers,
    })
}

/// Triggers on one table: timing, the events that fire it, and its
/// `pg_get_triggerdef` (which names the target function).
pub async fn load_triggers(pool: &PgPool, schema: &str, table: &str) -> Result<Vec<TriggerInfo>> {
    let rows = sqlx::query(
        "SELECT t.tgname AS name,
                c.relname AS table,
                CASE
                    WHEN t.tgtype & 2  <> 0 THEN 'BEFORE'
                    WHEN t.tgtype & 64 <> 0 THEN 'INSTEAD OF'
                    ELSE 'AFTER'
                END AS timing,
                string_agg(DISTINCT CASE
                    WHEN t.tgtype & 4  <> 0 THEN 'INSERT'
                    WHEN t.tgtype & 8  <> 0 THEN 'DELETE'
                    WHEN t.tgtype & 16 <> 0 THEN 'UPDATE'
                    ELSE 'TRUNCATE'
                END, ', ') AS event,
                pg_get_triggerdef(t.oid) AS function
         FROM pg_trigger t
         JOIN pg_class c ON c.oid = t.tgrelid
         JOIN pg_namespace n ON n.oid = c.relnamespace
         WHERE n.nspname = $1 AND c.relname = $2 AND NOT t.tgisinternal
         GROUP BY t.tgname, c.relname, t.tgtype, t.oid, t.tgrelid
         ORDER BY t.tgname",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| TriggerInfo {
            name: r.get("name"),
            table: r.get("table"),
            event: r.get("event"),
            timing: r.get("timing"),
            function: r.get("function"),
        })
        .collect())
}
