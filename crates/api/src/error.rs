use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

#[derive(Debug)]
pub struct StartupError {
    message: String,
}

impl StartupError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for StartupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for StartupError {}

#[derive(Debug)]
pub enum ApiError {
    InvalidSlug,
    InvalidArticle(String),
    Unauthorized,
    NotFound,
    Conflict,
    Internal,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSlug => formatter.write_str("invalid_slug"),
            Self::InvalidArticle(message) => formatter.write_str(message),
            Self::Unauthorized => formatter.write_str("unauthorized"),
            Self::NotFound => formatter.write_str("article_not_found"),
            Self::Conflict => formatter.write_str("slug_exists"),
            Self::Internal => formatter.write_str("internal_error"),
        }
    }
}

impl std::error::Error for ApiError {}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error, message) = match &self {
            Self::InvalidSlug => (StatusCode::BAD_REQUEST, "invalid_slug", None),
            Self::InvalidArticle(detail) => (
                StatusCode::BAD_REQUEST,
                "invalid_article",
                Some(detail.clone()),
            ),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized", None),
            Self::NotFound => (StatusCode::NOT_FOUND, "article_not_found", None),
            Self::Conflict => (StatusCode::CONFLICT, "slug_exists", None),
            Self::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error", None),
        };

        if matches!(self, Self::Internal) {
            tracing::error!("request failed with an internal database error");
        }

        let mut response = (status, Json(ErrorBody { error, message })).into_response();

        if status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }

        response
    }
}
