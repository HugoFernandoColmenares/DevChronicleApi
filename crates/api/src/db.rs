use std::path::PathBuf;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow};
use sqlx::{FromRow, Row, Sqlite, SqlitePool, Transaction};

use crate::error::{ApiError, StartupError};
use crate::models::{ArticleBody, ArticleMetadata, WriteArticle};

pub async fn prepare_database(database_url: &str) -> Result<SqlitePool, StartupError> {
    let pool = connect_pool(database_url).await?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|error| StartupError::new(format!("migrations failed: {error}")))?;
    Ok(pool)
}

async fn connect_pool(database_url: &str) -> Result<SqlitePool, StartupError> {
    let path = sqlite_file_path(database_url)?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|error| {
                StartupError::new(format!("create directory {}: {error}", parent.display()))
            })?;
        }
    }

    let options = SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));

    SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .map_err(|error| StartupError::new(format!("open {}: {error}", path.display())))
}

fn sqlite_file_path(database_url: &str) -> Result<PathBuf, StartupError> {
    let rest = database_url
        .strip_prefix("sqlite:")
        .ok_or_else(|| StartupError::new("DATABASE_URL must start with sqlite:"))?;
    let path = rest.split_once('?').map(|(path, _)| path).unwrap_or(rest);
    let path = path.strip_prefix("//").unwrap_or(path);
    if path.is_empty() || path == ":memory:" {
        return Err(StartupError::new(
            "DATABASE_URL must point at a SQLite file",
        ));
    }
    Ok(PathBuf::from(path))
}

struct ArticleRow {
    slug: String,
    title: String,
    author: String,
    published_on: String,
    excerpt: String,
    markdown: String,
    tags: String,
}

impl FromRow<'_, SqliteRow> for ArticleRow {
    fn from_row(row: &SqliteRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            slug: row.try_get("slug")?,
            title: row.try_get("title")?,
            author: row.try_get("author")?,
            published_on: row.try_get("published_on")?,
            excerpt: row.try_get("excerpt")?,
            markdown: row.try_get("markdown")?,
            tags: row.try_get("tags")?,
        })
    }
}

impl ArticleRow {
    fn into_body(self) -> ArticleBody {
        let mut tags = split_tags(&self.tags);
        tags.sort();
        ArticleBody {
            metadata: ArticleMetadata {
                slug: self.slug,
                title: self.title,
                author: self.author,
                date: self.published_on,
                tags,
                excerpt: self.excerpt,
            },
            markdown: self.markdown,
        }
    }
}

fn split_tags(raw: &str) -> Vec<String> {
    if raw.is_empty() {
        Vec::new()
    } else {
        raw.split('\u{1f}').map(str::to_string).collect()
    }
}

const ARTICLE_SQL: &str = r#"
SELECT
    a.slug,
    a.title,
    a.author,
    a.published_on,
    a.excerpt,
    a.markdown,
    COALESCE((
        SELECT group_concat(t.tag, char(31))
        FROM article_tags t
        WHERE t.article_id = a.id
    ), '') AS tags
FROM articles a
WHERE (?1 = 0 OR a.published = 1)
  AND (?2 = '' OR a.slug = ?2)
  AND (
        ?3 = ''
        OR EXISTS (
            SELECT 1
            FROM article_tags f
            WHERE f.article_id = a.id AND f.tag = ?3
        )
  )
ORDER BY a.published_on DESC, a.slug ASC
"#;

pub async fn list_published(
    pool: &SqlitePool,
    tag: Option<&str>,
) -> Result<Vec<ArticleMetadata>, ApiError> {
    let rows = fetch_rows(pool, true, None, tag).await?;
    Ok(rows
        .into_iter()
        .map(|row| row.into_body().metadata)
        .collect())
}

pub async fn get_published(pool: &SqlitePool, slug: &str) -> Result<ArticleBody, ApiError> {
    fetch_one(pool, true, slug).await
}

pub async fn get_any(pool: &SqlitePool, slug: &str) -> Result<ArticleBody, ApiError> {
    fetch_one(pool, false, slug).await
}

async fn fetch_one(
    pool: &SqlitePool,
    published_only: bool,
    slug: &str,
) -> Result<ArticleBody, ApiError> {
    let mut rows = fetch_rows(pool, published_only, Some(slug), None).await?;
    rows.pop()
        .map(ArticleRow::into_body)
        .ok_or(ApiError::NotFound)
}

async fn fetch_rows(
    pool: &SqlitePool,
    published_only: bool,
    slug: Option<&str>,
    tag: Option<&str>,
) -> Result<Vec<ArticleRow>, ApiError> {
    sqlx::query_as::<_, ArticleRow>(ARTICLE_SQL)
        .bind(if published_only { 1 } else { 0 })
        .bind(slug.unwrap_or(""))
        .bind(tag.unwrap_or(""))
        .fetch_all(pool)
        .await
        .map_err(map_db)
}

pub async fn create(pool: &SqlitePool, article: &WriteArticle) -> Result<ArticleBody, ApiError> {
    let mut tx = pool.begin().await.map_err(map_db)?;
    insert_article(&mut tx, article, false).await?;
    tx.commit().await.map_err(map_db)?;
    get_any(pool, &article.slug).await
}

pub async fn update(
    pool: &SqlitePool,
    slug: &str,
    article: &WriteArticle,
) -> Result<ArticleBody, ApiError> {
    let mut tx = pool.begin().await.map_err(map_db)?;
    let result = sqlx::query(
        r#"
        UPDATE articles
        SET title = ?1,
            author = ?2,
            published_on = ?3,
            excerpt = ?4,
            markdown = ?5,
            published = ?6,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
        WHERE slug = ?7
        "#,
    )
    .bind(&article.title)
    .bind(&article.author)
    .bind(&article.date)
    .bind(&article.excerpt)
    .bind(&article.markdown)
    .bind(published_flag(article.published))
    .bind(slug)
    .execute(tx.as_mut())
    .await
    .map_err(map_db)?;

    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }

    replace_tags(&mut tx, slug, &article.tags).await?;
    tx.commit().await.map_err(map_db)?;
    get_any(pool, slug).await
}

pub async fn delete(pool: &SqlitePool, slug: &str) -> Result<(), ApiError> {
    let result = sqlx::query("DELETE FROM articles WHERE slug = ?1")
        .bind(slug)
        .execute(pool)
        .await
        .map_err(map_db)?;

    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(())
}

/// Inserts the article only when the slug is new.
///
/// A second seed must not overwrite edits made by the author. `ON CONFLICT DO
/// UPDATE` would replace those edits, so the seed uses `DO NOTHING`.
pub async fn insert_if_absent(
    tx: &mut Transaction<'_, Sqlite>,
    article: &WriteArticle,
) -> Result<bool, ApiError> {
    insert_article(tx, article, true).await
}

async fn insert_article(
    tx: &mut Transaction<'_, Sqlite>,
    article: &WriteArticle,
    ignore_conflict: bool,
) -> Result<bool, ApiError> {
    let statement = if ignore_conflict {
        r#"
        INSERT INTO articles (slug, title, author, published_on, excerpt, markdown, published)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ON CONFLICT(slug) DO NOTHING
        "#
    } else {
        r#"
        INSERT INTO articles (slug, title, author, published_on, excerpt, markdown, published)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#
    };

    let result = sqlx::query(statement)
        .bind(&article.slug)
        .bind(&article.title)
        .bind(&article.author)
        .bind(&article.date)
        .bind(&article.excerpt)
        .bind(&article.markdown)
        .bind(published_flag(article.published))
        .execute(tx.as_mut())
        .await
        .map_err(map_db)?;

    if result.rows_affected() == 0 {
        return Ok(false);
    }

    let article_id = result.last_insert_rowid();
    for tag in &article.tags {
        sqlx::query("INSERT INTO article_tags (article_id, tag) VALUES (?1, ?2)")
            .bind(article_id)
            .bind(tag)
            .execute(tx.as_mut())
            .await
            .map_err(map_db)?;
    }
    Ok(true)
}

async fn replace_tags(
    tx: &mut Transaction<'_, Sqlite>,
    slug: &str,
    tags: &[String],
) -> Result<(), ApiError> {
    sqlx::query(
        "DELETE FROM article_tags WHERE article_id = (SELECT id FROM articles WHERE slug = ?1)",
    )
    .bind(slug)
    .execute(tx.as_mut())
    .await
    .map_err(map_db)?;

    for tag in tags {
        sqlx::query(
            r#"
            INSERT INTO article_tags (article_id, tag)
            SELECT id, ?2 FROM articles WHERE slug = ?1
            "#,
        )
        .bind(slug)
        .bind(tag)
        .execute(tx.as_mut())
        .await
        .map_err(map_db)?;
    }
    Ok(())
}

fn published_flag(published: bool) -> i64 {
    if published {
        1
    } else {
        0
    }
}

fn map_db(error: sqlx::Error) -> ApiError {
    if let sqlx::Error::Database(database_error) = &error {
        if database_error.is_unique_violation() {
            return ApiError::Conflict;
        }
    }
    tracing::error!(error = %error, "sqlite request failed");
    ApiError::Internal
}
