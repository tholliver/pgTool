/// A failed query, split into what the collapsed status strip shows (one
/// line) and what the expanded panel shows (full Postgres diagnostic:
/// code, message, detail, hint). Falls back to the raw error string for
/// non-database failures (connection loss, cancellation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryError {
    pub headline: String,
    pub detail: String,
}

impl QueryError {
    pub fn from_anyhow(err: &anyhow::Error) -> Self {
        if let Some(sqlx::Error::Database(db_err)) = err.downcast_ref::<sqlx::Error>() {
            let message = db_err.message().to_string();
            let code_prefix = db_err.code().map(|c| format!("{c}: ")).unwrap_or_default();
            let mut lines = vec![format!("ERROR: {code_prefix}{message}")];

            // sqlx 0.8: `DatabaseError::try_downcast_ref` (sqlx-core error.rs);
            // PgDatabaseError carries the extra DETAIL/HINT fields.
            if let Some(pg) = db_err.try_downcast_ref::<sqlx::postgres::PgDatabaseError>() {
                if let Some(detail) = pg.detail() {
                    lines.push(format!("DETAIL: {detail}"));
                }
                if let Some(hint) = pg.hint() {
                    lines.push(format!("HINT: {hint}"));
                }
            }
            QueryError {
                headline: message,
                detail: lines.join("\n"),
            }
        } else {
            let msg = err.to_string();
            QueryError {
                headline: msg.clone(),
                detail: msg,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PgDatabaseError is not constructible outside the driver, so this
    /// harness exercises the non-DB fallback path with DB-shaped text and
    /// documents the expected DB-path shape (ERROR:/DETAIL:/HINT: lines)
    /// for verification against a live error.
    fn db_shaped_error(code: &str, message: &str) -> anyhow::Error {
        anyhow::anyhow!("{code}: {message}")
    }

    #[test]
    fn non_database_error_uses_message_as_both_fields() {
        let err = anyhow::anyhow!("query was cancelled");
        let qe = QueryError::from_anyhow(&err);
        assert_eq!(qe.headline, "query was cancelled");
        assert_eq!(qe.detail, "query was cancelled");
    }

    #[test]
    fn headline_never_contains_newlines() {
        // Whatever the source, the collapsed strip must stay one line —
        // this is the property the UI actually depends on.
        let err = db_shaped_error("42601", "syntax error at or near \"on\"");
        let qe = QueryError::from_anyhow(&err);
        assert!(
            !qe.headline.contains('\n'),
            "headline must render on a single status-strip line"
        );
        assert_eq!(qe.detail, qe.headline, "fallback path mirrors headline");
    }
}
