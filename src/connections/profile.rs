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
        match &self.database {
            Some(db) if !db.is_empty() => db.as_str(),
            _ => "postgres",
        }
    }

    pub fn connection_url(&self, password: &str) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}?sslmode={}",
            self.username,
            password,
            self.host,
            self.port,
            self.maintenance_database(),
            self.ssl_mode.as_str(),
        )
    }

    /// Build a connection URL targeting a *specific* database on this same
    /// server, reusing the profile's host/port/user/password/ssl settings.
    pub fn connection_url_for_database(&self, password: &str, database: &str) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}?sslmode={}",
            self.username,
            password,
            self.host,
            self.port,
            database,
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
