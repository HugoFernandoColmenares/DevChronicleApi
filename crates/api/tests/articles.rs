use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use devchronicle_api::{build_app, prepare_database, seed_from_content, Config};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

const TOKEN: &str = "test-author-token";

struct Running {
    app: axum::Router,
    url: String,
    _dir: tempfile::TempDir,
}

async fn boot() -> Running {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("journal.db");
    let url = sqlite_url(&database);
    let app = build_app(&Config {
        database_url: url.clone(),
        author_token: TOKEN.to_string(),
        bind: "127.0.0.1:0".to_string(),
    })
    .await
    .unwrap();

    Running {
        app,
        url,
        _dir: dir,
    }
}

fn sqlite_url(path: &Path) -> String {
    format!(
        "sqlite://{}?mode=rwc",
        path.to_string_lossy().replace('\\', "/")
    )
}

async fn send(app: &axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    if bytes.is_empty() {
        return (status, Value::Null);
    }
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

fn request(method: &str, uri: &str, token: Option<&str>, body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if let Some(body) = body {
        builder = builder.header("content-type", "application/json");
        builder
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    }
}

fn article(slug: &str, published: bool) -> Value {
    json!({
        "slug": slug,
        "title": "A title",
        "author": "Hugo Colmenares",
        "date": "2026-09-10",
        "tags": ["angular", "architecture"],
        "excerpt": "Short excerpt",
        "markdown": "## Hello\n\nBody",
        "published": published
    })
}

#[tokio::test]
async fn health_reports_ok() {
    let running = boot().await;
    let (status, body) = send(&running.app, request("GET", "/api/v1/health", None, None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "status": "ok" }));
}

#[tokio::test]
async fn hidden_drafts_invalid_slugs_auth_and_conflicts() {
    let running = boot().await;

    let (status, body) = send(
        &running.app,
        request(
            "POST",
            "/api/v1/articles",
            Some(TOKEN),
            Some(article("draft-note", false)),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["metadata"]["slug"], "draft-note");
    assert!(body["markdown"].as_str().unwrap().contains("Hello"));

    let (status, listed) = send(&running.app, request("GET", "/api/v1/articles", None, None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed, json!([]));

    let (status, missing) = send(
        &running.app,
        request("GET", "/api/v1/articles/draft-note", None, None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(missing, json!({ "error": "article_not_found" }));

    let (status, _) = send(
        &running.app,
        request(
            "POST",
            "/api/v1/articles",
            Some(TOKEN),
            Some(article("public-note", true)),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, listed) = send(
        &running.app,
        request("GET", "/api/v1/articles?tag=angular", None, None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["slug"], "public-note");
    assert_eq!(listed[0]["tags"], json!(["angular", "architecture"]));
    assert!(listed[0].get("markdown").is_none());

    let (status, bad_slug) = send(
        &running.app,
        request(
            "POST",
            "/api/v1/articles",
            Some(TOKEN),
            Some(article("Not A Slug", true)),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(bad_slug["error"], "invalid_slug");

    let (status, _) = send(
        &running.app,
        request("GET", "/api/v1/articles/Not_A_Slug", None, None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, unauthorized) = send(
        &running.app,
        request(
            "POST",
            "/api/v1/articles",
            None,
            Some(article("another-note", true)),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(unauthorized, json!({ "error": "unauthorized" }));

    let (status, _) = send(
        &running.app,
        request(
            "POST",
            "/api/v1/articles",
            Some("wrong-token"),
            Some(article("another-note", true)),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, conflict) = send(
        &running.app,
        request(
            "POST",
            "/api/v1/articles",
            Some(TOKEN),
            Some(article("public-note", true)),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict, json!({ "error": "slug_exists" }));
}

#[tokio::test]
async fn delete_removes_the_row_and_its_tags() {
    let running = boot().await;
    send(
        &running.app,
        request(
            "POST",
            "/api/v1/articles",
            Some(TOKEN),
            Some(article("temp-note", true)),
        ),
    )
    .await;

    let pool = prepare_database(&running.url).await.unwrap();
    let tags_before: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM article_tags WHERE tag = 'angular'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(tags_before, 1);

    let (status, body) = send(
        &running.app,
        request("DELETE", "/api/v1/articles/temp-note", Some(TOKEN), None),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);

    let tags_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM article_tags")
        .fetch_one(&pool)
        .await
        .unwrap();
    let articles_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM articles")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tags_after, 0);
    assert_eq!(articles_after, 0);

    let (status, _) = send(
        &running.app,
        request("GET", "/api/v1/articles/temp-note", None, None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = send(
        &running.app,
        request("DELETE", "/api/v1/articles/temp-note", Some(TOKEN), None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn update_replaces_fields_and_can_publish() {
    let running = boot().await;
    send(
        &running.app,
        request(
            "POST",
            "/api/v1/articles",
            Some(TOKEN),
            Some(article("rewrite-me", false)),
        ),
    )
    .await;

    let mut replacement = article("rewrite-me", true);
    replacement["title"] = json!("Rewritten");
    replacement["markdown"] = json!("Only the body");
    replacement["tags"] = json!(["journal"]);
    replacement["date"] = json!("2026-09-12");

    let (status, body) = send(
        &running.app,
        request(
            "PUT",
            "/api/v1/articles/rewrite-me",
            Some(TOKEN),
            Some(replacement),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["metadata"]["title"], "Rewritten");
    assert_eq!(body["metadata"]["date"], "2026-09-12");
    assert_eq!(body["metadata"]["tags"], json!(["journal"]));
    assert_eq!(body["markdown"], "Only the body");

    let (status, listed) = send(&running.app, request("GET", "/api/v1/articles", None, None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed[0]["title"], "Rewritten");

    let mut mismatched = article("other-slug", true);
    mismatched["slug"] = json!("other-slug");
    let (status, error) = send(
        &running.app,
        request(
            "PUT",
            "/api/v1/articles/rewrite-me",
            Some(TOKEN),
            Some(mismatched),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "invalid_article");
}

#[tokio::test]
async fn seed_is_idempotent_and_keeps_author_edits() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path());

    let database = dir.path().join("seed.db");
    let url = sqlite_url(&database);
    let pool = prepare_database(&url).await.unwrap();

    let first = seed_from_content(&pool, dir.path()).await.unwrap();
    assert_eq!(first.inserted, 1);
    assert_eq!(first.skipped, 0);

    let markdown: String =
        sqlx::query_scalar("SELECT markdown FROM articles WHERE slug = 'hello-world'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(markdown, "Body text");
    assert!(!markdown.contains("title:"));

    let second = seed_from_content(&pool, dir.path()).await.unwrap();
    assert_eq!(second.inserted, 0);
    assert_eq!(second.skipped, 1);

    sqlx::query("UPDATE articles SET title = 'Edited by the author' WHERE slug = 'hello-world'")
        .execute(&pool)
        .await
        .unwrap();

    let third = seed_from_content(&pool, dir.path()).await.unwrap();
    assert_eq!(third.inserted, 0);
    assert_eq!(third.skipped, 1);

    let title: String = sqlx::query_scalar("SELECT title FROM articles WHERE slug = 'hello-world'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(title, "Edited by the author");

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM articles")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn seed_loads_the_frontend_journal_without_frontmatter() {
    let content = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../frontend/public/content");
    if !content.join("index.json").is_file() {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let url = sqlite_url(&dir.path().join("real.db"));
    let pool = prepare_database(&url).await.unwrap();
    let report = seed_from_content(&pool, &content).await.unwrap();
    assert_eq!(report.inserted, 4);

    let again = seed_from_content(&pool, &content).await.unwrap();
    assert_eq!(again.inserted, 0);
    assert_eq!(again.skipped, 4);

    let markdown: String =
        sqlx::query_scalar("SELECT markdown FROM articles WHERE slug = 'welcome-to-devchronicle'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(markdown.starts_with("DevChronicle is a local"));
    assert!(!markdown.starts_with("---"));

    let app = build_app(&Config {
        database_url: url,
        author_token: TOKEN.to_string(),
        bind: "127.0.0.1:0".to_string(),
    })
    .await
    .unwrap();

    let (status, listed) = send(&app, request("GET", "/api/v1/articles", None, None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed.as_array().unwrap().len(), 4);
    assert_eq!(listed[0]["slug"], "markdown-as-source-of-truth");
    assert_eq!(listed[3]["slug"], "welcome-to-devchronicle");

    let (status, article) = send(
        &app,
        request(
            "GET",
            "/api/v1/articles/angular-signals-in-practice",
            None,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(article["metadata"]["date"], "2026-09-10");
    assert_eq!(
        article["metadata"]["tags"],
        json!(["angular", "architecture"])
    );
    assert!(!article["markdown"].as_str().unwrap().contains("title:"));
}

fn write_fixture(dir: &Path) {
    std::fs::write(
        dir.join("index.json"),
        r#"[
          {
            "slug": "hello-world",
            "title": "Hello",
            "author": "Hugo Colmenares",
            "date": "2026-09-08",
            "tags": ["journal"],
            "excerpt": "Hi"
          }
        ]"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("hello-world.md"),
        "---\ntitle: Hello\nauthor: Hugo Colmenares\ndate: 2026-09-08\ntags: [journal]\nexcerpt: Hi\n---\n\nBody text\n",
    )
    .unwrap();
}
