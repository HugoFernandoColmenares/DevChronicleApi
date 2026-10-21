use serde::{Deserialize, Serialize};

use crate::error::ApiError;

const MAX_SLUG_LEN: usize = 80;
const MAX_TITLE_LEN: usize = 180;
const MAX_AUTHOR_LEN: usize = 120;
const MAX_EXCERPT_LEN: usize = 500;
const MAX_MARKDOWN_LEN: usize = 400_000;
const MAX_TAGS: usize = 12;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ArticleMetadata {
    pub slug: String,
    pub title: String,
    pub author: String,
    pub date: String,
    pub tags: Vec<String>,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ArticleBody {
    pub metadata: ArticleMetadata,
    pub markdown: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WriteArticle {
    pub slug: String,
    pub title: String,
    pub author: String,
    pub date: String,
    pub tags: Vec<String>,
    pub excerpt: String,
    pub markdown: String,
    pub published: bool,
}

impl WriteArticle {
    pub fn normalize(&mut self) -> Result<(), ApiError> {
        self.slug = self.slug.trim().to_string();
        self.title = self.title.trim().to_string();
        self.author = self.author.trim().to_string();
        self.date = self.date.trim().to_string();
        self.excerpt = self.excerpt.trim().to_string();
        self.tags = self
            .tags
            .iter()
            .map(|tag| tag.trim().to_string())
            .filter(|tag| !tag.is_empty())
            .collect();

        if !is_valid_slug(&self.slug) || self.slug.chars().count() > MAX_SLUG_LEN {
            return Err(ApiError::InvalidSlug);
        }
        require_text(&self.title, "title", MAX_TITLE_LEN)?;
        require_text(&self.author, "author", MAX_AUTHOR_LEN)?;
        if !is_valid_iso_date(&self.date) {
            return Err(ApiError::InvalidArticle(
                "date must be a real calendar day in YYYY-MM-DD form".into(),
            ));
        }
        if self.excerpt.chars().count() > MAX_EXCERPT_LEN {
            return Err(ApiError::InvalidArticle("excerpt is too long".into()));
        }
        if self.markdown.chars().count() > MAX_MARKDOWN_LEN {
            return Err(ApiError::InvalidArticle("markdown is too long".into()));
        }
        if self.tags.len() > MAX_TAGS {
            return Err(ApiError::InvalidArticle(
                "an article can have at most 12 tags".into(),
            ));
        }

        let mut seen = Vec::new();
        for tag in &self.tags {
            if !is_valid_slug(tag) {
                return Err(ApiError::InvalidArticle(format!(
                    "tag '{tag}' does not match the slug pattern"
                )));
            }
            if seen.iter().any(|existing: &String| existing == tag) {
                return Err(ApiError::InvalidArticle(format!(
                    "tag '{tag}' is duplicated"
                )));
            }
            seen.push(tag.clone());
        }

        Ok(())
    }
}

pub fn is_valid_slug(slug: &str) -> bool {
    let mut previous_was_hyphen = true;
    if slug.is_empty() {
        return false;
    }

    for character in slug.chars() {
        match character {
            'a'..='z' | '0'..='9' => previous_was_hyphen = false,
            '-' if !previous_was_hyphen => previous_was_hyphen = true,
            _ => return false,
        }
    }

    !previous_was_hyphen
}

pub fn is_valid_iso_date(value: &str) -> bool {
    let mut parts = value.split('-');
    let (Some(year), Some(month), Some(day), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    if year.len() != 4 || month.len() != 2 || day.len() != 2 {
        return false;
    }

    let Ok(year) = year.parse::<i32>() else {
        return false;
    };
    let Ok(month) = month.parse::<u32>() else {
        return false;
    };
    let Ok(day) = day.parse::<u32>() else {
        return false;
    };
    if !(1..=9999).contains(&year) {
        return false;
    }

    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => return false,
    };
    (1..=max_day).contains(&day)
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn require_text(value: &str, field: &str, max: usize) -> Result<(), ApiError> {
    if value.is_empty() {
        return Err(ApiError::InvalidArticle(format!("{field} is required")));
    }
    if value.chars().count() > max {
        return Err(ApiError::InvalidArticle(format!("{field} is too long")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_matches_the_frontend_pattern() {
        assert!(is_valid_slug("angular-signals-in-practice"));
        assert!(is_valid_slug("a"));
        assert!(!is_valid_slug("Angular"));
        assert!(!is_valid_slug("-bad"));
        assert!(!is_valid_slug("bad-"));
        assert!(!is_valid_slug("bad_slug"));
        assert!(!is_valid_slug("bad slug"));
        assert!(!is_valid_slug(""));
    }

    #[test]
    fn date_rejects_impossible_days() {
        assert!(is_valid_iso_date("2026-09-10"));
        assert!(is_valid_iso_date("2024-02-29"));
        assert!(!is_valid_iso_date("2026-02-29"));
        assert!(!is_valid_iso_date("2026-13-01"));
        assert!(!is_valid_iso_date("2026-9-10"));
    }
}
