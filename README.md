# DevChronicleApi

DevChronicleApi is the backend of DevChronicle, a markdown journal. It is a Rust workspace that serves the catalog and raw Markdown from SQLite. The Angular client compiles that Markdown into HTML. The workspace currently contains one crate, `crates/api`, exposed as a library and two binaries.

- `devchronicle-api` reads the environment and calls `serve`.
- `seed` loads a content directory once.

`build_app` and `serve` live in the library so a later Tauri shell can start the same router. The Docker image packages these two binaries only. The desktop shell is described in `docs/files/hybrid-web-desktop.md`. The client is [DevChronicleApp](https://github.com/HugoFernandoColmenares/DevChronicleApp). The Compose stack is [DevChronicle](https://github.com/HugoFernandoColmenares/DevChronicle).

```text
backend/
├── Cargo.toml
├── Dockerfile
├── .env.example
├── crates/api/
│   ├── migrations/0001_articles.sql
│   ├── requests.http
│   ├── src/          configuration, models, SQL, routes, seed, binaries
│   └── tests/articles.rs
└── docs/
```

On startup the process opens the SQLite file and creates it when missing. It sets `PRAGMA foreign_keys = ON` and `journal_mode = WAL`, runs the migrations embedded in the binary, and listens for HTTP. The pool allows at most five connections.

## Behavior

- The public contract is JSON metadata plus raw Markdown. Slugs match `^[a-z0-9]+(?:-[a-z0-9]+)*$`.
- An invalid slug on `POST` or `PUT` returns `400`. On `GET`, an invalid or unpublished slug returns `404` with `{ "error": "article_not_found" }`.
- Writes require `Authorization: Bearer <AUTHOR_TOKEN>`. The process exits when that variable is missing or empty.
- The seed uses `ON CONFLICT(slug) DO NOTHING`, so a second run does not overwrite author edits.
- Migrations ship inside the binary through `sqlx::migrate!`. The Docker image does not need a `migrations` directory at runtime.
- CORS is permissive so `ng serve` can call the API directly. Behind nginx, the browser uses one origin.
- `DATABASE_URL` accepts `sqlite:///data/devchronicle.db?mode=rwc` and `sqlite://./data/devchronicle.db?mode=rwc`. The process opens the file for reading and writing and creates it when needed.

## Requirements

- Stable Rust and Cargo.
- On Windows, the default `x86_64-pc-windows-msvc` host needs the Visual C++ Build Tools (`link.exe`) and the Windows SDK. The Docker image does not: `rust:1-bookworm` provides the C toolchain SQLx uses to compile SQLite.
- SQLite is compiled into the crate. A separate database server is not required.

## Configuration

Copy the template and set the token. `.env` is ignored by git. This crate does not load that file by itself. The variables must be present in the process environment, whether that is PowerShell, a shell, or Docker.

```powershell
copy .env.example .env
```

| Variable | Required | Purpose |
| :--- | :--- | :--- |
| `AUTHOR_TOKEN` | Yes, for the API | Author secret. The seed binary does not read it. |
| `DATABASE_URL` | Yes | SQLite file. |
| `BIND` | No | Defaults to `0.0.0.0:8080`. Local runs should use `127.0.0.1:8080`. |
| `CONTENT_DIR` | No | Directory containing `index.json` and the `.md` files. When unset, `seed` looks for `frontend/public/content`. |

Example from `backend/` in PowerShell:

```powershell
$env:AUTHOR_TOKEN = "replace-with-a-private-token"
$env:DATABASE_URL = "sqlite://./data/devchronicle.db?mode=rwc"
$env:BIND = "127.0.0.1:8080"
$env:CONTENT_DIR = "..\frontend\public\content"
```

## Run

From `backend/`:

```powershell
cargo run --bin seed
cargo run --bin devchronicle-api
```

Run the seed by hand. It does not run on every startup. The first run inserts the sample entries. A later run reports `inserted 0` and leaves existing titles and Markdown unchanged.

Health check:

```text
GET http://127.0.0.1:8080/api/v1/health
{ "status": "ok" }
```

`crates/api/requests.http` lists the remaining calls for a REST client.

## Contract

Base path: `/api/v1`.

| Method | Path | Auth | Result |
| :--- | :--- | :--- | :--- |
| `GET` | `/health` | No | `{ "status": "ok" }` |
| `GET` | `/articles` | No | Published entries, newest first. `?tag=` filters the list. |
| `GET` | `/articles/{slug}` | No | `{ metadata, markdown }` with no frontmatter. |
| `POST` | `/articles` | Bearer | `201` and the resource. `409` when the slug already exists. |
| `PUT` | `/articles/{slug}` | Bearer | Replaces title, author, date, tags, excerpt, Markdown, and `published`. |
| `DELETE` | `/articles/{slug}` | Bearer | `204`. Tag rows are removed by the foreign-key cascade. |

Create and update body:

```json
{
  "slug": "angular-signals-in-practice",
  "title": "Angular signals in practice",
  "author": "Hugo Colmenares",
  "date": "2026-09-10",
  "tags": ["angular", "architecture"],
  "excerpt": "...",
  "markdown": "## Inputs without decorators\n",
  "published": true
}
```

`published: false` stores a draft. Drafts are omitted from the public list and from public reads. `date` is the editorial date in `YYYY-MM-DD` form. Tags use the same pattern as slugs and are returned in order.

Common error codes are `invalid_slug`, `invalid_article`, `unauthorized`, `article_not_found`, `slug_exists`, and `internal_error`. Malformed JSON also returns `400` with `invalid_article`.

## Tests

```powershell
cargo test
```

The HTTP suite covers hidden drafts, an invalid slug, `401`, `409`, tag removal on delete, updates, and an idempotent seed, including the real frontend content.

With the variables above, exercise a running process from another terminal:

```powershell
curl.exe -s http://127.0.0.1:8080/api/v1/health
curl.exe -s http://127.0.0.1:8080/api/v1/articles
curl.exe -s http://127.0.0.1:8080/api/v1/articles/angular-signals-in-practice
curl.exe -s -X POST http://127.0.0.1:8080/api/v1/articles -H "Authorization: Bearer replace-with-a-private-token" -H "Content-Type: application/json" --data-binary "@body.json"
```

A `POST` without a token returns `401`. Repeating a slug returns `409`. `DELETE` of a new slug returns `204`, and the list no longer includes it.

The checks below are in `docs/screenshots/`. `api-health.png` is `GET /api/v1/health`. `api-articles.png` is the list of the four seeded entries. A browser may pretty-print the JSON. The API itself returns compact JSON.

![Health response](docs/screenshots/api-health.png)

![Seeded article list](docs/screenshots/api-articles.png)

## Docker

The build context is this directory. The image installs `devchronicle-api` and `seed` in `/usr/local/bin`, drops to the `chronicle` user, and expects the database volume at `/data`.

```powershell
docker build -t devchronicle-api .
docker run --rm -p 8080:8080 -e AUTHOR_TOKEN=replace-with-a-private-token -e DATABASE_URL=sqlite:///data/devchronicle.db?mode=rwc -e BIND=0.0.0.0:8080 -v chronicle-data:/data devchronicle-api
```

The image build context is only this directory, so the seed process cannot see `frontend/` unless that directory is mounted:

```powershell
docker run --rm -e DATABASE_URL=sqlite:///data/devchronicle.db?mode=rwc -e CONTENT_DIR=/seed/content -v chronicle-data:/data -v ${PWD}/../frontend/public/content:/seed/content:ro --entrypoint /usr/local/bin/seed devchronicle-api
```

Compose in the orchestration repository performs the same mount. Nginx and the Angular client belong to that stack, not to this crate.

## Later work

An editing screen, accounts, an editorial draft workflow, search, RSS, image uploads, and the Tauri window are outside this cut. When the desktop shell arrives, `backend/app/` will depend on this library and call `serve` on `127.0.0.1`.

## Author

Hugo Fernando Colmenares. Public repositories: [HugoFernandoColmenares](https://github.com/HugoFernandoColmenares?tab=repositories).

This repository is [DevChronicleApi](https://github.com/HugoFernandoColmenares/DevChronicleApi).

| Project | Role |
| :--- | :--- |
| [DevChronicleApi](https://github.com/HugoFernandoColmenares/DevChronicleApi) | Rust API and SQLite |
| [DevChronicleApp](https://github.com/HugoFernandoColmenares/DevChronicleApp) | Angular frontend |
| [DevChronicle](https://github.com/HugoFernandoColmenares/DevChronicle) | Docker Compose orchestration |

## License

Private project. All rights reserved unless the repository owner states otherwise.
