# DevChronicle API

Motor HTTP del journal. Sirve el catálogo y el Markdown desde SQLite, y deja la compilación del HTML en el cliente Angular. La cáscara de escritorio no entra en este corte: el diseño está en `docs/files/hybrid-web-desktop.md` y el plan de este servicio está en `docs/files/mvp-rust-sqlite-docker.md`, en la raíz del repositorio.

## Qué quedó armado

El workspace tiene un solo crate, `crates/api`. Es biblioteca y dos binarios:

- `devchronicle-api` lee el entorno y llama a `serve`.
- `seed` carga `frontend/public/content` una vez.

`build_app` y `serve` viven en la biblioteca para que, más adelante, Tauri pueda arrancar el mismo router sin copiar SQL ni rutas. Docker empaqueta solo estos dos binarios.

```text
backend/
├── Cargo.toml
├── Dockerfile
├── .env.example
├── crates/api/
│   ├── migrations/0001_articles.sql
│   ├── requests.http
│   ├── src/          config, modelos, SQL, rutas, semilla, binarios
│   └── tests/articles.rs
└── docs/
```

Al arrancar, el proceso abre el archivo SQLite (lo crea si no existe), aplica `PRAGMA foreign_keys = ON` y `journal_mode = WAL`, corre las migraciones embebidas y escucha HTTP. El pool tiene como máximo 5 conexiones.

## Decisiones de este corte

- El contrato público es JSON de metadatos más Markdown crudo. El slug sigue el patrón del cliente: `^[a-z0-9]+(?:-[a-z0-9]+)*$`.
- Un slug inválido en `POST` o `PUT` responde `400`. En `GET`, un slug inválido o no publicado responde `404` con `{ "error": "article_not_found" }`, igual que una entrada que no existe.
- La escritura exige `Authorization: Bearer <AUTHOR_TOKEN>`. El token no tiene valor por defecto: si falta o está vacío, el proceso termina.
- La semilla usa `ON CONFLICT(slug) DO NOTHING`. El plan menciona `DO UPDATE`, pero también pide que un segundo pase no pise ediciones del autor. Se cumple la segunda regla: lo que ya está en la base se deja como está.
- Las migraciones viajan dentro del binario (`sqlx::migrate!`). La imagen de Docker no necesita el directorio `migrations` en tiempo de ejecución.
- CORS está abierto para que `ng serve` pueda llamar al API en local. Detrás de nginx, el navegador no cruza orígenes.
- `DATABASE_URL` acepta la forma del plan (`sqlite:///data/devchronicle.db?mode=rwc` o `sqlite://./data/devchronicle.db?mode=rwc`). El proceso ignora `mode` y abre el archivo en lectura y escritura, creándolo si hace falta.

## Requisitos

- Rust stable, con Cargo.
- En Windows, el host por defecto `x86_64-pc-windows-msvc` necesita las Build Tools de Visual C++ (`link.exe`) y el Windows SDK. Sin ese enlazador, Cargo no produce un binario. La imagen Docker no depende de eso: `rust:1-bookworm` trae el compilador de C que SQLx usa para SQLite.
- SQLx compila SQLite dentro del crate. No hace falta un servidor de base de datos.

## Configuración

Copia la plantilla y completa el token. El archivo `.env` está ignorado por git; esta carpeta no lo carga sola: las variables tienen que estar en el entorno del proceso (PowerShell, el shell o Docker).

```powershell
copy .env.example .env
```

| Variable | Obligatorio | Uso |
| :--- | :--- | :--- |
| `AUTHOR_TOKEN` | Sí, para el API | Secreto del autor. La semilla no lo usa. |
| `DATABASE_URL` | Sí | Archivo SQLite. |
| `BIND` | No | Por defecto `0.0.0.0:8080`. En local conviene `127.0.0.1:8080`. |
| `CONTENT_DIR` | No | Carpeta con `index.json` y los `.md`. Si falta, `seed` busca `frontend/public/content`. |

Ejemplo en PowerShell, desde `backend/`:

```powershell
$env:AUTHOR_TOKEN = "un-token-largo-y-secreto"
$env:DATABASE_URL = "sqlite://./data/devchronicle.db?mode=rwc"
$env:BIND = "127.0.0.1:8080"
$env:CONTENT_DIR = "..\frontend\public\content"
```

## Arranque

Desde `backend/`:

```powershell
cargo run --bin seed
cargo run --bin devchronicle-api
```

La semilla se ejecuta a mano, no en cada arranque. La primera vez inserta las cuatro entradas del frontend. La segunda informa `inserted 0` y no cambia títulos ni Markdown ya editados.

Salud del proceso:

```text
GET http://127.0.0.1:8080/api/v1/health
{ "status": "ok" }
```

`crates/api/requests.http` tiene el resto de llamadas para un cliente REST.

## Contrato

Base: `/api/v1`.

| Método | Ruta | Auth | Resultado |
| :--- | :--- | :--- | :--- |
| `GET` | `/health` | No | `{ "status": "ok" }` |
| `GET` | `/articles` | No | Publicadas, de la más reciente a la más antigua. `?tag=` filtra. |
| `GET` | `/articles/{slug}` | No | `{ metadata, markdown }` sin frontmatter. |
| `POST` | `/articles` | Bearer | `201` y el recurso. `409` si el slug ya existe. |
| `PUT` | `/articles/{slug}` | Bearer | Sustituye título, autor, fecha, etiquetas, extracto, Markdown y `published`. |
| `DELETE` | `/articles/{slug}` | Bearer | `204`. La clave foránea en cascada borra las etiquetas. |

El cuerpo de alta y edición:

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

`published: false` guarda un borrador. No sale en el listado ni en la lectura pública. `date` es la fecha editorial `YYYY-MM-DD`. Las etiquetas usan el mismo patrón que el slug y se devuelven ordenadas.

Errores habituales: `invalid_slug`, `invalid_article`, `unauthorized`, `article_not_found`, `slug_exists`, `internal_error`. Un JSON mal formado también responde `400` con `invalid_article`.

## Pruebas

```powershell
cargo test
```

La suite HTTP cubre borradores ocultos, slug inválido, `401`, `409`, borrado con cascada de etiquetas, edición y semilla idempotente, incluida la del contenido real del frontend.

Para probar el proceso en marcha, con las variables de arriba:

```powershell
cargo run --bin seed
cargo run --bin devchronicle-api
```

En otra terminal:

```powershell
curl.exe -s http://127.0.0.1:8080/api/v1/health
curl.exe -s http://127.0.0.1:8080/api/v1/articles
curl.exe -s http://127.0.0.1:8080/api/v1/articles/angular-signals-in-practice
curl.exe -s -X POST http://127.0.0.1:8080/api/v1/articles -H "Authorization: Bearer un-token-largo-y-secreto" -H "Content-Type: application/json" --data-binary "@cuerpo.json"
```

Un `POST` sin token tiene que responder `401`. Repetir el mismo slug, `409`. `DELETE` del slug nuevo, `204`, y el listado deja de mostrarlo.

Las capturas de esa comprobación están en `docs/screenshots/`: `api-health.png` es `GET /api/v1/health` y `api-articles.png` es el listado de las cuatro entradas sembradas. El navegador las muestra formateadas; el API responde JSON compacto.

## Docker

El contexto de build es esta carpeta. La imagen deja `devchronicle-api` y `seed` en `/usr/local/bin`, corre con el usuario `chronicle` y espera el volumen en `/data`.

```powershell
docker build -t devchronicle-api .
docker run --rm -p 8080:8080 -e AUTHOR_TOKEN=un-token-largo-y-secreto -e DATABASE_URL=sqlite:///data/devchronicle.db?mode=rwc -e BIND=0.0.0.0:8080 -v chronicle-data:/data devchronicle-api
```

La semilla dentro del contenedor no ve `frontend/`, porque el contexto de la imagen es solo `backend/`. Hay que montar el contenido:

```powershell
docker run --rm -e DATABASE_URL=sqlite:///data/devchronicle.db?mode=rwc -e CONTENT_DIR=/seed/content -v chronicle-data:/data -v ${PWD}/../frontend/public/content:/seed/content:ro --entrypoint /usr/local/bin/seed devchronicle-api
```

Eso corresponde al `docker compose run --rm api /usr/local/bin/seed` del plan, con el montaje que el contexto `./backend` no puede evitar. El `compose.yaml` de la raíz, nginx y el cliente Angular siguen siendo el paso de orquestación y no forman parte de este crate.

## Fuera de este corte

Pantalla de edición, cuentas, borradores con flujo editorial, búsqueda, RSS, imágenes y la ventana Tauri. Cuando llegue el escritorio, `backend/app/` dependerá de esta biblioteca y llamará a `serve` en `127.0.0.1`.
