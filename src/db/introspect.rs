use anyhow::Result;
use sqlx::{PgPool, Row};

use super::models::{ColumnInfo, DatabaseInfo, SchemaInfo, TableInfo};

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
        schemas.push(SchemaInfo {
            name: schema_name,
            tables,
        });
    }

    Ok(schemas)
}

async fn load_tables(pool: &PgPool, schema: &str) -> Result<Vec<TableInfo>> {
    let table_rows = sqlx::query(
        "SELECT table_name FROM information_schema.tables
         WHERE table_schema = $1 AND table_type = 'BASE TABLE'
         ORDER BY table_name",
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
