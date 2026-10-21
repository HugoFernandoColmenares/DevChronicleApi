CREATE TABLE articles (
    id          INTEGER PRIMARY KEY,
    slug        TEXT NOT NULL UNIQUE,
    title       TEXT NOT NULL,
    author      TEXT NOT NULL,
    published_on TEXT NOT NULL,
    excerpt     TEXT NOT NULL DEFAULT '',
    markdown    TEXT NOT NULL,
    published   INTEGER NOT NULL DEFAULT 1 CHECK (published IN (0, 1)),
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE article_tags (
    article_id INTEGER NOT NULL REFERENCES articles(id) ON DELETE CASCADE,
    tag        TEXT NOT NULL,
    PRIMARY KEY (article_id, tag)
);

CREATE INDEX idx_articles_published_on ON articles (published, published_on DESC);
CREATE INDEX idx_article_tags_tag ON article_tags (tag);
