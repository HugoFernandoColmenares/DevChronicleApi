use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::request::Parts;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::db;
use crate::error::ApiError;
use crate::models::{is_valid_slug, ArticleBody, ArticleMetadata, WriteArticle};
use crate::AppState;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/articles", get(list_articles).post(create_article))
        .route(
            "/api/v1/articles/{slug}",
            get(get_article).put(update_article).delete(delete_article),
        )
        .fallback(unknown_route)
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(state)
}

#[derive(Deserialize)]
struct ListQuery {
    tag: Option<String>,
}

struct Author;

impl FromRequestParts<AppState> for Author {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header_value = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .ok_or(ApiError::Unauthorized)?;

        let token = header_value
            .strip_prefix("Bearer ")
            .or_else(|| header_value.strip_prefix("bearer "))
            .ok_or(ApiError::Unauthorized)?;

        if !tokens_match(token, &state.author_token) {
            return Err(ApiError::Unauthorized);
        }
        Ok(Author)
    }
}

fn tokens_match(provided: &str, expected: &str) -> bool {
    let provided = provided.as_bytes();
    let expected = expected.as_bytes();
    if provided.len() != expected.len() {
        return false;
    }
    let mut difference = 0_u8;
    for (left, right) in provided.iter().zip(expected.iter()) {
        difference |= left ^ right;
    }
    difference == 0
}

async fn health(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    sqlx::query("SELECT 1")
        .execute(&state.pool)
        .await
        .map_err(|_| ApiError::Internal)?;
    Ok(Json(json!({ "status": "ok" })))
}

async fn list_articles(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<ArticleMetadata>>, ApiError> {
    let tag = match query
        .tag
        .as_deref()
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
    {
        Some(tag) if is_valid_slug(tag) => Some(tag.to_string()),
        Some(_) => {
            return Err(ApiError::InvalidArticle(
                "tag does not match the slug pattern".into(),
            ))
        }
        None => None,
    };

    let articles = db::list_published(&state.pool, tag.as_deref()).await?;
    Ok(Json(articles))
}

async fn get_article(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<ArticleBody>, ApiError> {
    if !is_valid_slug(&slug) {
        return Err(ApiError::NotFound);
    }
    let article = db::get_published(&state.pool, &slug).await?;
    Ok(Json(article))
}

async fn create_article(
    State(state): State<AppState>,
    _author: Author,
    body: Result<Json<WriteArticle>, axum::extract::rejection::JsonRejection>,
) -> Result<
    (
        StatusCode,
        [(header::HeaderName, String); 1],
        Json<ArticleBody>,
    ),
    ApiError,
> {
    let article = parse_write(body)?;
    let created = db::create(&state.pool, &article).await?;
    Ok((
        StatusCode::CREATED,
        [(
            header::LOCATION,
            format!("/api/v1/articles/{}", created.metadata.slug),
        )],
        Json(created),
    ))
}

async fn update_article(
    State(state): State<AppState>,
    _author: Author,
    Path(slug): Path<String>,
    body: Result<Json<WriteArticle>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<ArticleBody>, ApiError> {
    if !is_valid_slug(&slug) {
        return Err(ApiError::InvalidSlug);
    }
    let article = parse_write(body)?;
    if article.slug != slug {
        return Err(ApiError::InvalidArticle(
            "slug in the body must match the slug in the path".into(),
        ));
    }
    let updated = db::update(&state.pool, &slug, &article).await?;
    Ok(Json(updated))
}

async fn delete_article(
    State(state): State<AppState>,
    _author: Author,
    Path(slug): Path<String>,
) -> Result<StatusCode, ApiError> {
    if !is_valid_slug(&slug) {
        return Err(ApiError::InvalidSlug);
    }
    db::delete(&state.pool, &slug).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn unknown_route() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, Json(json!({ "error": "not_found" })))
}

fn parse_write(
    body: Result<Json<WriteArticle>, axum::extract::rejection::JsonRejection>,
) -> Result<WriteArticle, ApiError> {
    let Json(mut article) =
        body.map_err(|_| ApiError::InvalidArticle("request body must be a JSON article".into()))?;
    article.normalize()?;
    Ok(article)
}
