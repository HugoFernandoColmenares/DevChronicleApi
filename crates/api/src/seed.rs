use std::path::{Path, PathBuf};

use serde::Deserialize;
use sqlx::SqlitePool;

use crate::db;
use crate::error::ApiError;
use crate::models::WriteArticle;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedReport {
    pub inserted: usize,
    pub skipped: usize,
}

#[derive(Debug)]
pub struct SeedError {
    message: String,
}

impl SeedError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for SeedError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SeedError {}

#[derive(Debug, Deserialize)]
struct IndexEntry {
    slug: String,
    title: String,
    author: String,
    date: String,
    tags: Vec<String>,
    excerpt: String,
}

pub fn resolve_content_dir() -> Result<PathBuf, SeedError> {
    let mut candidates = Vec::new();
    if let Ok(value) = std::env::var("CONTENT_DIR") {
        let value = value.trim();
        if !value.is_empty() {
            candidates.push(PathBuf::from(value));
        }
    }

    candidates.push(PathBuf::from("/seed/content"));
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    candidates.push(manifest.join("../../frontend/public/content"));
    candidates.push(PathBuf::from("frontend/public/content"));
    candidates.push(PathBuf::from("../frontend/public/content"));
    candidates.push(PathBuf::from("../../frontend/public/content"));
    candidates.push(PathBuf::from("../../../frontend/public/content"));

    let mut tried = Vec::new();
    for candidate in candidates {
        if candidate.join("index.json").is_file() {
            return Ok(candidate);
        }
        tried.push(candidate.display().to_string());
    }

    Err(SeedError::new(format!(
        "could not find index.json. Set CONTENT_DIR. Looked in: {}",
        tried.join(", ")
    )))
}

pub async fn seed_from_content(
    pool: &SqlitePool,
    content_dir: &Path,
) -> Result<SeedReport, SeedError> {
    let articles = load_articles(content_dir)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| SeedError::new(format!("begin seed transaction: {error}")))?;

    let mut report = SeedReport {
        inserted: 0,
        skipped: 0,
    };

    for article in &articles {
        let inserted = db::insert_if_absent(&mut tx, article)
            .await
            .map_err(|error| SeedError::new(seed_write_message(&article.slug, error)))?;
        if inserted {
            report.inserted += 1;
        } else {
            report.skipped += 1;
        }
    }

    tx.commit()
        .await
        .map_err(|error| SeedError::new(format!("commit seed: {error}")))?;
    Ok(report)
}

fn seed_write_message(slug: &str, error: ApiError) -> String {
    format!("seed article '{slug}': {error}")
}

fn load_articles(content_dir: &Path) -> Result<Vec<WriteArticle>, SeedError> {
    let index_path = content_dir.join("index.json");
    let raw = std::fs::read_to_string(&index_path)
        .map_err(|error| SeedError::new(format!("read {}: {error}", index_path.display())))?;
    let entries: Vec<IndexEntry> = serde_json::from_str(&raw)
        .map_err(|error| SeedError::new(format!("parse {}: {error}", index_path.display())))?;

    let mut articles = Vec::with_capacity(entries.len());
    for entry in entries {
        let markdown_path = content_dir.join(format!("{}.md", entry.slug));
        let markdown_raw = std::fs::read_to_string(&markdown_path).map_err(|error| {
            SeedError::new(format!("read {}: {error}", markdown_path.display()))
        })?;

        let mut article = WriteArticle {
            slug: entry.slug,
            title: entry.title,
            author: entry.author,
            date: entry.date,
            tags: entry.tags,
            excerpt: entry.excerpt,
            markdown: strip_frontmatter(&markdown_raw),
            published: true,
        };
        article
            .normalize()
            .map_err(|error| SeedError::new(format!("article '{}': {error}", article.slug)))?;
        articles.push(article);
    }

    Ok(articles)
}

fn strip_frontmatter(raw: &str) -> String {
    let trimmed = raw
        .trim_start_matches('\u{feff}')
        .trim_start_matches(['\r', '\n']);
    let Some(after_open) = trimmed
        .strip_prefix("---\r\n")
        .or_else(|| trimmed.strip_prefix("---\n"))
    else {
        return trimmed.trim().to_string();
    };

    for marker in ["\r\n---\r\n", "\n---\n", "\r\n---\n", "\n---\r\n"] {
        if let Some(index) = after_open.find(marker) {
            return after_open[index + marker.len()..].trim().to_string();
        }
    }

    trimmed.trim().to_string()
}
