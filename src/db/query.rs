use anyhow::{Context, Result};
use futures_util::TryStreamExt;
use sqlx::{Column, PgPool, Row, TypeInfo, ValueRef};

use super::models::QueryResult;

pub async fn execute_query(pool: &PgPool, sql: &str) -> Result<QueryResult> {
    // Run over the simple (text) query protocol: PostgreSQL formats EVERY
    // data type as readable text itself — timestamps, timestamptz, numeric,
    // arrays, jsonb, ranges, enums, even extension/custom types. There are
    // no per-type decodings to get wrong; what you see here is what
    // psql/pgAdmin would show.
    let mut stream = sqlx::raw_sql(sql).fetch(pool);

    let mut columns: Vec<String> = Vec::new();
    let mut column_types: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();

    while let Some(row) = stream.try_next().await.context("query failed")? {
        if columns.is_empty() {
            columns = row
                .columns()
                .iter()
                .map(|c| c.name().to_string())
                .collect();
            column_types = row
                .columns()
                .iter()
                .map(|c| c.type_info().name().to_string())
                .collect();
        }

        let mut values = Vec::with_capacity(row.columns().len());
        for i in 0..row.columns().len() {
            // Values arrive in text format; NULL arrives as a distinct null
            // sentinel (not an empty string).
            let val = row.try_get_raw(i)?;
            if val.is_null() {
                values.push("NULL".to_string());
            } else {
                match val.as_str() {
                    Ok(s) => values.push(s.to_owned()),
                    Err(_) => values.push(
                        String::from_utf8_lossy(val.as_bytes().unwrap_or_default()).into_owned(),
                    ),
                }
            }
        }
        rows.push(values);
    }

    let rows_affected = rows.len() as u64;

    Ok(QueryResult {
        columns,
        column_types,
        rows,
        rows_affected,
    })
}

/// Quote a PostgreSQL identifier for safe inline embedding. Identifiers are
/// introspected from the schema catalogs (never raw user input), and wrapping
/// them in double quotes handles reserved words and mixed case; embedded
/// quotes are escaped by doubling.
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// UPDATE one cell of one row, identified by equality on the row's primary
/// key. The new value and the PK values are bound as parameters so Postgres
/// infers the column types from context (no client-side casting, no
/// injection); identifiers come from the catalog. Returns rows affected.
pub async fn execute_cell_update(
    pool: &PgPool,
    schema: &str,
    table: &str,
    column: &str,
    value: &str,
    identity: &[(String, String)],
) -> Result<u64> {
    debug_assert!(
        !identity.is_empty(),
        "a single-row UPDATE needs a WHERE predicate"
    );

    let table_ident = format!("{}.{}", quote_ident(schema), quote_ident(table));
    let set_clause = format!("{} = $1", quote_ident(column));
    let where_clause = identity
        .iter()
        .enumerate()
        .map(|(i, (col, _))| format!("{} = ${}", quote_ident(col), i + 2))
        .collect::<Vec<_>>()
        .join(" AND ");
    let sql = format!("UPDATE {table_ident} SET {set_clause} WHERE {where_clause}");

    let mut q = sqlx::query(&sql).bind(value);
    for (_, v) in identity {
        q = q.bind(v);
    }

    let res = q.execute(pool).await.context("cell update failed")?;
    Ok(res.rows_affected() as u64)
}

/// Run `EXPLAIN (FORMAT JSON, ANALYZE ?) <sql>` and return the plan as a
/// JSON-encoded string (Postgres emits a one-row, one-column result holding
/// the serialized plan tree). Uses the same text-protocol streaming as
/// [`execute_query`], so the JSON arrives exactly as Postgres formats it.
///
/// ANALYZE actually executes the statement — for data-modifying statements the
/// caller refuses it up front so it can never write real data.
pub async fn execute_explain(pool: &PgPool, sql: &str, analyze: bool) -> Result<String> {
    let options = if analyze {
        "FORMAT JSON, ANALYZE true, BUFFERS true, SUMMARY true"
    } else {
        "FORMAT JSON, ANALYZE false"
    };
    let explain_sql = format!("EXPLAIN ({options})\n{sql}");
    let mut stream = sqlx::raw_sql(&explain_sql).fetch(pool);

    let mut plan = String::new();
    while let Some(row) = stream.try_next().await.context("explain failed")? {
        let val = row.try_get_raw(0)?;
        if !val.is_null() {
            match val.as_str() {
                Ok(s) => plan.push_str(s),
                Err(_) => plan.push_str(&String::from_utf8_lossy(val.as_bytes().unwrap_or_default())),
            }
        }
    }

    if plan.trim().is_empty() {
        anyhow::bail!("EXPLAIN returned no plan");
    }
    Ok(plan)
}

/// UPDATE several columns of one row in a single statement. Only the columns
/// listed in `updates` appear in the SET clause; the row is targeted by
/// equality on its primary key (the identity pairs). Values and PK values are
/// bound as parameters, so Postgres infers types from context; identifiers
/// come from the catalog. Returns rows affected.
pub async fn execute_row_update(
    pool: &PgPool,
    schema: &str,
    table: &str,
    updates: &[(String, String)],
    identity: &[(String, String)],
) -> Result<u64> {
    debug_assert!(
        !identity.is_empty(),
        "a single-row UPDATE needs a WHERE predicate"
    );
    debug_assert!(
        !updates.is_empty(),
        "an UPDATE needs at least one SET column"
    );

    let table_ident = format!("{}.{}", quote_ident(schema), quote_ident(table));
    let set_clause = updates
        .iter()
        .enumerate()
        .map(|(i, (col, _))| format!("{} = ${}", quote_ident(col), i + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let where_clause = identity
        .iter()
        .enumerate()
        .map(|(i, (col, _))| {
            format!(
                "{} = ${}",
                quote_ident(col),
                i + 1 + updates.len()
            )
        })
        .collect::<Vec<_>>()
        .join(" AND ");
    let sql = format!("UPDATE {table_ident} SET {set_clause} WHERE {where_clause}");

    let mut q = sqlx::query(&sql);
    for (_, v) in updates {
        q = q.bind(v);
    }
    for (_, v) in identity {
        q = q.bind(v);
    }

    let res = q.execute(pool).await.context("row update failed")?;
    Ok(res.rows_affected() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live smoke test against the local database (skips silently when
    /// DATABASE_URL is unset or unreachable, e.g. in CI).
    #[tokio::test]
    async fn renders_all_types_as_text() {
        dotenvy::dotenv().ok();
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL not set");
            return;
        };

        let Ok(pool) = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
        else {
            eprintln!("skipping: cannot reach database");
            return;
        };

        let result = execute_query(
            &pool,
            "SELECT
                now()::timestamptz          AS ts_tz,
                '2026-08-23'::timestamp     AS ts_plain,
                '2026-08-23'::date          AS d,
                '12:34:56'::time            AS t,
                interval '1 day 2 hours'    AS iv,
                12.34::numeric              AS num,
                array[1,2,3]                AS arr,
                '{\"a\": 1}'::jsonb         AS js,
                gen_random_uuid()           AS uid,
                true                        AS flag,
                'hello world'               AS txt,
                ''                          AS empty_txt,
                NULL                        AS nada",
        )
        .await
        .expect("query should succeed");

        assert_eq!(result.columns.len(), 13);
        let row = &result.rows[0];

        // Timestamps must never render as NULL anymore.
        assert_ne!(row[0], "NULL", "timestamptz must render");
        assert_eq!(row[1], "2026-08-23 00:00:00");
        assert_eq!(row[2], "2026-08-23");
        assert_eq!(row[3], "12:34:56");
        assert!(row[4].contains("day"), "interval: {}", row[4]);
        assert_eq!(row[5], "12.34", "numeric keeps precision");
        assert_eq!(row[6], "{1,2,3}", "arrays render like psql");
        assert!(row[7].contains("\"a\""), "jsonb: {}", row[7]);
        assert!(row[8].contains('-'), "uuid: {}", row[8]);
        assert_eq!(row[9], "t", "booleans render like psql");
        assert_eq!(row[10], "hello world");
        assert_eq!(row[11], "", "empty string stays distinct from NULL");
        assert_eq!(row[12], "NULL");

        pool.close().await;
    }

    /// Live smoke test for EXPLAIN (skips silently when DATABASE_URL is unset
    /// or unreachable). The plan must decode as JSON and, with ANALYZE, be a
    /// real (non-null) run.
    #[tokio::test]
    async fn explain_returns_parseable_json() {
        dotenvy::dotenv().ok();
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL not set");
            return;
        };

        let Ok(pool) = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
        else {
            eprintln!("skipping: cannot reach database");
            return;
        };

        let plan = execute_explain(&pool, "SELECT 42", false)
            .await
            .expect("plain EXPLAIN should succeed");
        let value: serde_json::Value =
            serde_json::from_str(&plan).expect("plain EXPLAIN yield must be JSON");

        let analyzed = execute_explain(&pool, "SELECT 42", true)
            .await
            .expect("EXPLAIN ANALYZE should succeed");
        serde_json::from_str::<serde_json::Value>(&analyzed).expect("ANALYZE plan must be JSON");
        assert!(
            !analyzed.is_empty() && value.is_array(),
            "plan should be a JSON array"
        );

        pool.close().await;
    }
}
