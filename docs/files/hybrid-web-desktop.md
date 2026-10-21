# Aplicación híbrida: web y escritorio

Este documento es la arquitectura de `backend/`: el workspace Rust que comparte el API con la cáscara de escritorio. Compara Electron y Tauri y fija Tauri 2 dentro de esta carpeta. No forma parte del corte web descrito en `docs/files/mvp-rust-sqlite-docker.md`, en la raíz del repositorio.

La recomendación es Tauri 2. El API Axum y SQLite ya son Rust y viven en `backend/crates/api`; Tauri también es Rust, así que el dominio se comparte en ese crate. Electron sigue siendo viable, con un coste de tamaño y de un runtime de más. El cliente que la ventana carga está en `frontend/` y obedece `frontend/docs/files/design_guidelines.md`.

## 1. Qué significa híbrida aquí

Hay un solo cliente Angular y un solo contrato HTTP `/api/v1`. Cambian el proceso que hospeda ese cliente y el archivo SQLite que responde.

| Superficie | Quién pinta Angular | Dónde vive SQLite | Cómo se entrega |
| :--- | :--- | :--- | :--- |
| Web | nginx, dentro de Compose | volumen `chronicle-data` | `docker compose up` |
| Escritorio | ventana nativa del sistema | directorio de datos de la aplicación | instalador de Windows, macOS o Linux |

Son dos bases distintas. El escritorio no abre el volumen de Docker y el sitio no abre el archivo del usuario. Sincronizarlas es otro producto (cuentas, conflictos, borrados) y queda fuera de este análisis.

El escritorio útil para este journal es un motor local: al abrir la ventana, el mismo API escucha en `127.0.0.1` y Angular habla con `/api/v1` igual que en la web. Un escritorio que solo fuera un navegador apuntando a un servidor remoto no aprovecha Rust ni SQLite en la máquina del autor.

## 2. Lo que ya encaja con una cáscara de escritorio

El cliente actual ayuda a las dos opciones:

- Es un SPA. La ventana carga `index.html` y el router de Angular resuelve `/` y `/post/:slug`.
- `ContentFetcherService` ya concentra el HTTP. Con `apiBasePath` en `/api/v1`, la misma build sirve para nginx y para el loopback local.
- El Markdown se compila en el cliente (`marked`, DOMPurify, Highlight.js). El proceso nativo no necesita un motor de plantillas.
- La copia al portapapeles usa `navigator.clipboard` en un origen seguro. `http://127.0.0.1` lo es.
- SweetAlert2 y la paleta Civic Terminal viven en CSS. No dependen de diálogos nativos.

El plan de orquestación pide, por esta razón, que `backend/crates/api` sea biblioteca más binario: `build_router` y `serve` en `src/lib.rs`. El contenedor `api` de Compose y el proceso Tauri llaman a la misma función.

Dos detalles del cliente hay que cerrarlos antes de empaquetar, y el MVP ya los marca:

- Las fuentes salen de Google Fonts. En un escritorio sin red la lectura pierde Inter y JetBrains Mono. Hay que vendorizarlas.
- `<base href="/">` y los chunks con ruta absoluta funcionan si la ventana abre `http://127.0.0.1:<puerto>/`. Cargar el bundle con `file://` o con un protocolo propio rompe el router y los imports en diferido.

## 3. Electron

Electron junta Chromium y Node en un `BrowserWindow`. Angular se carga ahí. El API Rust no entra en ese proceso: se empaqueta como sidecar y Electron lo arranca.

```text
DevChronicle.exe (Electron)
├── Chromium          pinta el bundle Angular
├── Node              ciclo de vida, diálogos, auto-update
└── devchronicle-api  proceso hijo, Axum en 127.0.0.1
        └── %APPDATA%/DevChronicle/devchronicle.db
```

Qué se aprovecha de Rust: el binario del API y el archivo SQLite, sin cambios de SQL. Qué no se aprovecha: la cáscara. Node y Chromium son un segundo stack al lado del crate.

Viabilidad para este repositorio:

- Angular 22 entra sin adaptadores. En desarrollo la ventana puede apuntar a `ng serve`; en producción, al origen del sidecar.
- El contrato HTTP se mantiene. `ContentFetcherService` no aprende IPC.
- El instalador de Windows ronda las decenas o el centenar de megabytes porque incluye Chromium. La RAM de una ventana vacía suele quedar muy por encima de una vista WebView2.
- Hay que vigilar el hijo: si el API termina, la ventana muestra el error de red que ya define el MVP; al cerrar la ventana hay que matar el hijo para no dejar el puerto ocupado.
- `nodeIntegration` permanece desactivado y `contextIsolation` activo. Con el API en loopback, el preload puede limitarse a saber el puerto. No hace falta exponer `fs` al renderer: SQLite tiene un solo dueño, el proceso Rust.
- El equipo de build pasa a ser Node más Rust. El pipeline de Docker no sirve para firmar el instalador.

Electron es una vía sólida si hace falta un Chromium idéntico en Windows, macOS y Linux, o una API de navegador que WebView2 o WebKit no cubran. Este journal no usa ninguna de esas APIs.

## 4. Tauri

Tauri 2 es una cáscara en Rust. La ventana usa el motor del sistema: WebView2 en Windows, WKWebView en macOS y WebKitGTK en Linux. El crate de Tauri puede depender de la biblioteca `backend` y llamar a `serve` dentro del mismo proceso.

```text
DevChronicle (binario Tauri, Rust)
├── ventana WebView2 / WKWebView / WebKitGTK
│     └── bundle Angular
└── tokio
      └── serve(bind 127.0.0.1:puerto, sqlite de app data)
```

Un solo proceso, un solo pool de SQLite, el mismo router que Docker. Angular sigue viendo HTTP. No hay comandos Tauri para el catálogo ni el artículo: inventarlos duplicaría `routes.rs` y obligaría al cliente a tener dos transports.

Arranque:

1. Resolver el directorio de datos de la aplicación y la ruta del `.db`.
2. Elegir un puerto libre en loopback.
3. Lanzar `serve` en el runtime de Tokio, sin bloquear el hilo de la interfaz.
4. Abrir la ventana en `http://127.0.0.1:<puerto>/`.
5. Al cerrar la ventana, cortar el servidor y soltar el pool.

El binario de Tauri puede servir también los estáticos de Angular en ese mismo origen (`/` el SPA, `/api/` el router). Así `<base href="/">`, la recarga de `/post/:slug` y el `apiBasePath` relativo coinciden con nginx. Esa es la prueba que decide si el spike es válido.

Viabilidad para este repositorio:

- El dominio Rust se escribe una vez. Docker usa el binario `devchronicle-api`. El escritorio usa la biblioteca.
- En Windows 10 y 11, WebView2 suele estar presente. Si falta, el instalador lleva el bootstrapper evergreen. No se empaqueta Chromium.
- El instalador queda en el orden de unos pocos megabytes más el bundle Angular, frente al runtime completo de Electron.
- Linux es el punto frágil: WebKitGTK tiene que estar en el sistema, y su versión cambia el aspecto y algún detalle de CSS. Conviene fijar la distribución de referencia cuando se empaquete Linux.
- Highlight.js, DOMPurify y SweetAlert2 corren dentro del webview. Hay que comprobar en el spike el `aria-label` de las vallas y la copia al portapapeles, porque el permiso del webview no es el de Chrome.
- Tauri 2 tiene permisos y capabilities. Como el renderer no recibe el sistema de archivos, la capability puede quedarse en el mínimo de la ventana.
- El usuario de este proyecto está en Windows. El primer paquete debe ser el `.msi` o el NSIS de Tauri. macOS y Linux esperan a que esa ventana pase el criterio de lectura.

## 5. Comparación

| Criterio | Electron | Tauri 2 |
| :--- | :--- | :--- |
| Reutiliza el crate Rust del API | Como proceso hijo | Como biblioteca en el mismo proceso |
| Motor de la interfaz | Chromium embebido | WebView del sistema |
| Runtimes además de Rust | Node y Chromium | El webview del sistema |
| Mismo contrato para Angular | Sí, HTTP en loopback | Sí, HTTP en loopback |
| Tamaño del instalador | Alto, incluye Chromium | Bajo, más el bundle Angular |
| Paridad visual entre sistemas | Alta | Alta en Windows; Linux depende de WebKitGTK |
| Encaja con el MVP ya escrito | Sí, lanzando el binario | Sí, y justifica el `lib.rs` del paso 1 |
| Trabajo nuevo de producto | Ciclo de vida del sidecar | Empaquetado, puerto y estáticos en el mismo origen |

## 6. Decisión

Seguir con Tauri 2 cuando el MVP web esté cerrado. Electron queda como alternativa si un spike demuestra que WebView2 rompe la columna de `70ch`, las vallas de código o la copia al portapapeles, y el arreglo en Tauri costara más que mantener Chromium.

Hasta entonces no se añade dependencia de Electron ni de `@tauri-apps/cli` al `package.json` de `frontend/`. El único preparativo dentro del MVP web es el crate de biblioteca descrito en `docs/files/mvp-rust-sqlite-docker.md`.

## 7. Forma objetivo

```text
backend/                         workspace Rust
├── Cargo.toml
├── Dockerfile                   imagen api para Compose
├── docs/
│   ├── files/                   este diseño
│   └── screenshots/             capturas de la ventana de escritorio
├── crates/api/                  biblioteca + binario devchronicle-api
└── app/                         cáscara Tauri 2
      └── abre http://127.0.0.1:<puerto>/
            └── sirve el bundle de frontend/dist
```

`backend/app/` aparece solo en el corte de escritorio. Su `Cargo.toml` depende de `backend/crates/api` por ruta. No se copia `db.rs` y no se crea un `src-tauri/` en la raíz ni dentro de `frontend/`. Compose sigue construyendo únicamente el binario del API.

Variables del proceso de escritorio, distintas de las de Compose:

| Variable | Valor de escritorio |
| :--- | :--- |
| `BIND` | `127.0.0.1:0` y luego la dirección real que asignó el sistema, o un puerto fijo documentado |
| `DATABASE_URL` | archivo bajo el directorio de datos de la app, modo `rwc` |
| `AUTHOR_TOKEN` | generado en el primer arranque y guardado junto a la base, con permisos del usuario |

El token local existe para no dejar el API de escritura abierto a cualquier proceso de la máquina. Angular embebido lo envía solo si más adelante hay pantalla de edición. En el primer corte de escritorio basta la lectura, igual que el sitio público.

La semilla del MVP (`seed`) se puede invocar una vez si la base está vacía, con las cuatro entradas de ejemplo. Un segundo arranque no pisa ediciones.

## 8. Spike que valida la elección

Una rama corta, después del MVP, con este criterio de éxito. Si falla, se reabre Electron antes de construir instaladores.

1. `ng build`, ejecutado en `frontend/`, y una ventana Tauri que carga ese bundle desde el propio `serve`.
2. `/` lista las entradas y el filtro por etiqueta cambia la grilla.
3. Recargar `/post/angular-signals-in-practice` sigue dentro de la ventana, con valla oscura, lenguaje y botón de copiar.
4. Cerrar la ventana libera el puerto y no deja un proceso Axum huérfano.
5. El archivo SQLite queda en el directorio de datos, sobrevive a un segundo arranque y no es el volumen de Docker.
6. Sin red, Inter y JetBrains Mono siguen activas.
7. La misma revisión, abierta en `http://localhost:8080/` vía Compose, se ve igual en lo que define la guía: lienzo `#FAFAFA`, consola `#1E293B`, acento `#0EA5E9`, medida `70ch`.

## 9. Pasos, cuando el spike pase

1. Añadir `backend/app/` con Tauri 2 y la dependencia de ruta a `backend/crates/api`.
2. Servir `frontend/dist/DevChronicle/browser` en el mismo origen que `/api/v1`.
3. Generar y persistir `AUTHOR_TOKEN` local.
4. Sembrar solo si la base no tiene filas.
5. Empaquetar Windows con el bootstrapper de WebView2.
6. Documentar en el README de `backend/` el comando de desarrollo y el de empaquetado. La ventana apunta al `serve` embebido; el build de Angular sale de `frontend/`.
7. Dejar macOS y Linux para un segundo paquete, con una distribución Linux fijada por el requisito de WebKitGTK.

## 10. Fuera de este híbrido

Cuentas compartidas entre la web y el escritorio, sincronización del `.db`, edición visual, bandeja del sistema, atajos globales y publicación en tiendas. El híbrido de este documento es la misma lectura en el navegador y en una ventana local, con un solo motor Rust.
