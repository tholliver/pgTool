use anyhow::{Context, Result};
use futures_util::TryStreamExt;
use sqlx::{Column, PgPool, Row, ValueRef};

use super::models::QueryResult;

pub async fn execute_query(pool: &PgPool, sql: &str) -> Result<QueryResult> {
    // Run over the simple (text) query protocol: PostgreSQL formats EVERY
    // data type as readable text itself — timestamps, timestamptz, numeric,
    // arrays, jsonb, ranges, enums, even extension/custom types. There are
    // no per-type decodings to get wrong; what you see here is what
    // psql/pgAdmin would show.
    let mut stream = sqlx::raw_sql(sql).fetch(pool);

    let mut columns: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();

    while let Some(row) = stream.try_next().await.context("query failed")? {
        if columns.is_empty() {
            columns = row
                .columns()
                .iter()
                .map(|c| c.name().to_string())
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
        rows,
        rows_affected,
    })
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
}
