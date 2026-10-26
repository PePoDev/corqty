#[derive(Clone)]
pub struct ApiConfig {
    pub bind_address: String,
    pub database_url: String,
    pub public_web_url: String,
    pub secure_cookies: bool,
    pub session_days: i64,
}

impl ApiConfig {
    pub fn from_env() -> Result<Self, std::env::VarError> {
        Ok(Self {
            bind_address: std::env::var("CORQTY_BIND_ADDRESS")
                .unwrap_or_else(|_| "0.0.0.0:8080".to_owned()),
            database_url: std::env::var("DATABASE_URL_API")?,
            public_web_url: std::env::var("CORQTY_PUBLIC_WEB_URL")
                .unwrap_or_else(|_| "http://localhost:3000".to_owned()),
            secure_cookies: std::env::var("CORQTY_SECURE_COOKIES")
                .map_or(true, |value| value != "false"),
            session_days: 30,
        })
    }

    #[must_use]
    pub fn for_tests() -> Self {
        Self {
            bind_address: "127.0.0.1:0".to_owned(),
            database_url: String::new(),
            public_web_url: "http://localhost:3000".to_owned(),
            secure_cookies: false,
            session_days: 30,
        }
    }
}
