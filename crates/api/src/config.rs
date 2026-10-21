use crate::error::StartupError;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub author_token: String,
    pub bind: String,
}

impl Config {
    pub fn from_env() -> Result<Self, StartupError> {
        let database_url = required_env("DATABASE_URL")?;
        let author_token = required_env("AUTHOR_TOKEN")?;
        let bind = std::env::var("BIND")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "0.0.0.0:8080".to_string());

        Ok(Self {
            database_url,
            author_token,
            bind,
        })
    }
}

pub fn database_url_from_env() -> Result<String, StartupError> {
    required_env("DATABASE_URL")
}

fn required_env(name: &str) -> Result<String, StartupError> {
    let value = std::env::var(name).map_err(|_| {
        StartupError::new(format!(
            "{name} is required and has no default in the repository"
        ))
    })?;
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(StartupError::new(format!("{name} is empty")));
    }
    Ok(value)
}
