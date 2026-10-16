use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SslMode {
    Disable,
    Prefer,
    Require,
    VerifyCa,
    VerifyFull,
}

impl SslMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Disable => "disable",
            Self::Prefer => "prefer",
            Self::Require => "require",
            Self::VerifyCa => "verify-ca",
            Self::VerifyFull => "verify-full",
        }
    }

    pub fn all() -> Vec<SslMode> {
        vec![
            SslMode::Disable,
            SslMode::Prefer,
            SslMode::Require,
            SslMode::VerifyCa,
            SslMode::VerifyFull,
        ]
    }
}

impl std::fmt::Display for SslMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionProfile {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    /// Empty/None = "browse all databases on this server" mode.
    /// Some("mydb") = classic single-database mode.
    #[serde(default)]
    pub database: Option<String>,
    pub username: String,
    pub ssl_mode: SslMode,
    /// Optional path to CA certificate for verify-ca/verify-full modes.
    #[serde(default)]
    pub ssl_ca_path: Option<String>,
    /// Only used in browse-all mode: database for the first server connection.
    #[serde(default)]
    pub initial_database: Option<String>,
}

/// Percent-encode a URL component (RFC 3986 unreserved set left untouched).
/// Avoids pulling in a URL-encoding crate for the few fields we build.
fn pct(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{:02X}", b),
        })
        .collect()
}

impl ConnectionProfile {
    /// True when this profile should list every database on the server
    /// instead of being scoped to a single one.
    pub fn browse_all_databases(&self) -> bool {
        self.database.as_deref().map(str::is_empty).unwrap_or(true)
    }

    /// The database to open the *initial* server connection against.
    /// Postgres requires connecting to *some* database, so when the user
    /// wants to browse everything, fall back to the "postgres" maintenance DB.
    pub fn maintenance_database(&self) -> &str {
        if let Some(db) = self.database.as_deref().filter(|d| !d.is_empty()) {
            return db;
        }
        if let Some(db) = self.initial_database.as_deref().filter(|d| !d.is_empty()) {
            return db;
        }
        "postgres"
    }

    pub fn connection_url(&self, password: &str) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}?sslmode={}",
            pct(&self.username),
            pct(password),
            self.host,
            self.port,
            pct(self.maintenance_database()),
            self.ssl_mode.as_str(),
        )
    }

    /// Build a connection URL targeting a *specific* database on this same
    /// server, reusing the profile's host/port/user/password/ssl settings.
    pub fn connection_url_for_database(&self, password: &str, database: &str) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}?sslmode={}",
            pct(&self.username),
            pct(password),
            self.host,
            self.port,
            pct(database),
            self.ssl_mode.as_str(),
        )
    }

    /// Generated display name for when name is empty.
    pub fn generated_name(&self) -> String {
        if self.name.is_empty() {
            format!("{}:{}", self.host, self.port)
        } else {
            self.name.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> ConnectionProfile {
        ConnectionProfile {
            id: "abc".into(),
            name: "n".into(),
            host: "db.example.com".into(),
            port: 5432,
            database: None,
            username: "admin".into(),
            ssl_mode: SslMode::Prefer,
            ssl_ca_path: None,
            initial_database: None,
        }
    }

    /// Minimal percent-decoder, mirrors `pct`.
    fn pct_decode(s: &str) -> String {
        let bytes = s.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                let hex = &s[i + 1..i + 3];
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8(out).expect("valid utf8")
    }

    /// Split `postgres://user:pass@host:port/db?sslmode=..` into
    /// (userinfo, rest). Panics if the userinfo is ambiguous (raw `@`).
    fn split_url(url: &str) -> (&str, &str) {
        let rest = url
            .strip_prefix("postgres://")
            .expect("postgres:// scheme");
        // Exactly one '@' => credentials and host are unambiguous.
        assert_eq!(rest.matches('@').count(), 1, "raw '@' in credentials: {url}");
        let idx = rest.find('@').expect("@ separator");
        (&rest[..idx], &rest[idx + 1..])
    }

    #[test]
    fn password_with_reserved_chars_is_percent_encoded_in_connection_url() {
        let pw = "p@ss:w/rd#1?";
        let url = profile().connection_url(pw);

        let (userinfo, rest) = split_url(&url);
        // No raw reserved character other than the ':' userinfo separator.
        for ch in ['@', '/', '#', '?'] {
            assert!(
                !userinfo.contains(ch),
                "raw '{}' in credentials part of {url}",
                ch
            );
        }
        // The ':' after the user is the separator, password itself has no raw ':'.
        let (user, pass) = userinfo.split_once(':').expect("user:pass");
        assert_eq!(pct_decode(user), "admin");
        assert_eq!(pct_decode(pass), pw);
        // Only one '?' overall - the sslmode query string.
        assert_eq!(url.matches('?').count(), 1, "unexpected '?' in {url}");
        assert!(!url.contains('#'), "raw '#' in {url}");
        // Host part must be intact (not shifted by a stray '@').
        assert!(rest.starts_with("db.example.com:5432/"), "got {rest}");
    }

    #[test]
    fn password_with_reserved_chars_is_percent_encoded_for_specific_database() {
        let pw = "p@ss:w/rd#1?";
        let url = profile().connection_url_for_database(pw, "my db/we?rd#1");

        let (userinfo, rest) = split_url(&url);
        let (user, pass) = userinfo.split_once(':').expect("user:pass");
        assert_eq!(pct_decode(user), "admin");
        assert_eq!(pct_decode(pass), pw);
        let (authority, query) = rest.split_once('?').expect("?sslmode");
        let (hostport, db) = authority.split_once('/').expect("/database");
        assert_eq!(hostport, "db.example.com:5432");
        assert_eq!(pct_decode(db), "my db/we?rd#1");
        assert_eq!(query, "sslmode=prefer");
        // Database segment must not leak raw reserved chars into the URL.
        assert!(
            !db.contains(['@', '/', '#', '?']),
            "raw reserved char in {db}"
        );
    }

    #[test]
    fn maintenance_database_prefers_database_then_initial_then_postgres() {
        let mut p = profile();
        assert_eq!(p.maintenance_database(), "postgres");

        p.initial_database = Some("defaultdb".into());
        assert_eq!(p.maintenance_database(), "defaultdb");

        p.database = Some("realdb".into());
        assert_eq!(p.maintenance_database(), "realdb");

        p.database = Some(String::new());
        p.initial_database = Some("defaultdb".into());
        assert_eq!(p.maintenance_database(), "defaultdb");

        p.initial_database = Some(String::new());
        assert_eq!(p.maintenance_database(), "postgres");
    }

    #[test]
    fn browse_all_databases_mode_detection() {
        let mut p = profile();
        assert!(p.browse_all_databases());
        p.database = Some(String::new());
        assert!(p.browse_all_databases());
        p.database = Some("mydb".into());
        assert!(!p.browse_all_databases());
    }

    #[test]
    fn legacy_json_without_initial_database_still_loads() {
        let json = r#"{
            "id": "x",
            "name": "Old Single DB",
            "host": "localhost",
            "port": 5432,
            "database": "mydb",
            "username": "postgres",
            "ssl_mode": "Disable"
        }"#;
        let p: ConnectionProfile = serde_json::from_str(json).expect("legacy json loads");
        assert_eq!(p.database.as_deref(), Some("mydb"));
        assert_eq!(p.initial_database, None);
        assert!(!p.browse_all_databases());
        assert_eq!(p.maintenance_database(), "mydb");
    }

    #[test]
    fn legacy_json_browse_all_without_database_field_loads() {
        let json = r#"{
            "id": "x",
            "name": "Browse All",
            "host": "localhost",
            "port": 5432,
            "username": "postgres",
            "ssl_mode": "Disable"
        }"#;
        let p: ConnectionProfile = serde_json::from_str(json).expect("json loads");
        assert_eq!(p.database, None);
        assert!(p.browse_all_databases());
    }
}
