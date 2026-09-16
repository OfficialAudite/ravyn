# ravyn

A self-hosted file hosting service, built entirely in Rust for performance and low
operating overhead. Alternative to [chibisafe](https://github.com/chibisafe/chibisafe)
and [zipline](https://github.com/diced/zipline).

## Architecture

```text
crates/
  ravyn-core/     domain types (File, Folder, ids, errors) — no framework dependencies
  ravyn-storage/  object storage (S3-compatible or local disk) via `object_store`
  ravyn-db/       Postgres access and migrations via `sqlx`
  ravyn-api/      HTTP API (axum): auth, uploads, folders, file serving
  ravyn-web/      frontend (Leptos, server-rendered + hydrated)
```

Each crate is independent and only depends on the ones below it in the list. This is
deliberate: you should be able to fork this repo and rip out or replace a whole layer
(swap `ravyn-web` for a different frontend, swap `ravyn-storage`'s backend, etc.)
without touching the rest.

## Running locally

You need Postgres reachable via `DATABASE_URL`.

```sh
# API server (auth, uploads, folders, file serving)
DATABASE_URL=postgres://localhost/ravyn STORAGE_ROOT=./data cargo run -p ravyn-api

# Web frontend (separate process, talks to the API server-side)
RAVYN_API_URL=http://127.0.0.1:3000 cargo leptos watch --project ravyn-web
```

Or via `docker compose up` — see [Docker](#docker) below.

The web UI never calls the API from the browser directly (no CORS setup needed): its
Leptos server functions run server-side, forward the session cookie to `ravyn-api` by
hand, and relay any `Set-Cookie` back. See `crates/ravyn-web/src/server_fns/`. File
uploads from the browser go through a plain HTML form (`POST /upload` on `ravyn-web`,
which proxies to the API) rather than a server function, so they keep working without
JS. Thumbnails are proxied too (`GET /preview/{id}` on `ravyn-web`) so your own
password-protected files still show a preview in your own dashboard — a plain `<img>`
pointed straight at `ravyn-api` would be a cross-origin request that never carries the
`ravyn-web` session cookie.

## Storage backends

Set `STORAGE_BACKEND=s3` to store uploaded files in S3 (or anything S3-compatible —
R2, MinIO, B2 via its S3 gateway):

```sh
STORAGE_BACKEND=s3 \
S3_BUCKET=my-bucket \
S3_REGION=auto \
S3_ENDPOINT=https://<account-id>.r2.cloudflarestorage.com \
S3_ACCESS_KEY_ID=... \
S3_SECRET_ACCESS_KEY=... \
cargo run -p ravyn-api
```

Leave `STORAGE_BACKEND` unset (or anything but `s3`) to store on local disk under
`STORAGE_ROOT` instead. Either way, uploads always go through `object_store`'s
multipart API (`crates/ravyn-storage/src/lib.rs`) rather than a single PUT — every S3
provider has its own limit on a single request's size, and multipart avoids all of
them uniformly, for files of any size.

Thumbnails (see below) are always local disk, under `THUMBNAIL_ROOT`, regardless of
where the original files live — they're small, read on every gallery view, and there's
no reason to round-trip them through S3.

Serving is always a passthrough, never a redirect: `GET /files/{id}` reads the bytes
from wherever they live and streams them back from `ravyn-api` itself. Point your own
domain at `ravyn-api` and every link — thumbnails, downloads, shared folders — stays on
that domain no matter which backend is storing the bytes.

## Auth

There's no self-service registration — accounts are provisioned by whoever runs the
server:

```sh
DATABASE_URL=postgres://localhost/ravyn cargo run -p ravyn-api -- create-user alice hunter2
```

- `POST /login` (`{"username", "password"}`) sets a session cookie, for the web UI.
- `POST /api-tokens` (`{"name"}`, requires a session cookie) mints an API token,
  returned once as `{"token"}`. This is what goes in a ShareX config.
- `POST /files` accepts either the session cookie or an `Authorization: Bearer
  <token>` header — ShareX only ever uses the latter.

An example ShareX custom uploader is in
[`contrib/sharex/ravyn.sxcu`](contrib/sharex/ravyn.sxcu) — import it, then paste your
token in place of `YOUR_API_TOKEN`.

## Folders, passwords, previews

- **Folders** are flat (no nesting) labels a file can belong to, each with its own
  optional password — `POST /folders`, `PUT /files/{id}/folder`. A folder's contents
  are viewable at `/f/{id}` on the web UI without logging in, gated by its password if
  it has one, the same sharing model as a single file link.
- **Passwords** can be set on an individual file too (`PUT /files/{id}/password`),
  independently of any folder it's in. A direct link to a password-protected file
  (`GET /files/{id}`) shows a small self-contained password prompt rather than the raw
  bytes, until the right `?password=` is supplied.
- The file *owner* always sees their own stuff — the password check is skipped
  whenever the request carries the owner's own session or API token
  (`routes::is_authorized` in `ravyn-api`).
- **Thumbnails** are generated at upload time for images (`crates/ravyn-api/src/routes/files.rs`,
  `generate_thumbnail`) and served separately from the original
  (`GET /files/{id}/thumbnail`), so the gallery grid never has to pull a full-size
  original just to render a 170px preview.
- The browse page's search box, type filter, and sort order are all client-side over
  the already-fetched file list (`crates/ravyn-web/src/dashboard.rs`) — there's no
  server-side filtering API, on the assumption that a self-hosted instance's file count
  stays in the range where that's fine.

## Web UI pages

The logged-in app is three pages under a shared layout (`DashboardLayout` in
`crates/ravyn-web/src/dashboard.rs`, using nested Leptos routes with an `<Outlet/>`):

- `/` — browse: folder sidebar, search/type/sort filters, the file grid.
- `/upload` — just the dropzone.
- `/settings` — account info, API token management (create/list/revoke — the token
  create endpoint used to be dashboard-only and had no way to see or revoke a token
  afterwards), and a read-only storage backend summary.

All three share one set of file/folder resources and mutation actions via
`provide_context`/`expect_context`, so an action taken on one page (say, deleting a
file) is reflected everywhere without a separate fetch per page. `/f/{id}` (the public
shared-folder view) stays a sibling top-level route, outside this layout, since it's
usable without logging in at all.

Multi-user accounts aren't supported yet — `/settings` has a placeholder note about it,
but there's currently exactly one owner per instance (`create-user` at the CLI).

## Docker

```sh
docker compose up -d --build
docker compose run --rm api create-user alice hunter2
```

Serves the API on `:3000` and the web UI on `:3001`. See `docker-compose.yml` for the
Postgres, storage-volume, and `RAVYN_PUBLIC_API_URL` wiring — that last one matters
specifically inside Docker: `ravyn-web` reaches `ravyn-api` over the compose network as
`http://api:3000`, but the *browser* needs the host-mapped `http://localhost:3000`
instead for thumbnails and download links, since it can't resolve the `api` hostname.

## Extending it

- **New storage backend**: add a variant to `StorageConfig` in
  `crates/ravyn-storage/src/config.rs`. `object_store` already has builders for GCS,
  Azure, and local disk in addition to S3.
- **New API route**: add a handler under `crates/ravyn-api/src/routes/` (in `files.rs`,
  `folders.rs`, or `account.rs`, whichever it's about) and register it in
  `routes::router()`.
- **New DB query**: add it to the relevant file under `crates/ravyn-db/src/` (e.g.
  `files.rs`, `folders.rs`, `api_tokens.rs`) as a method on `Db`.
- **Schema change**: add a new file under `crates/ravyn-db/migrations/`, run via
  `Db::migrate`.
- **New web page/action**: add a `#[server]` function under
  `crates/ravyn-web/src/server_fns/` (it forwards to the API and relays cookies for
  you). For a new *page*, add a component and a nested `<Route>` in `app.rs` — reach the
  shared file/folder state via `expect_context::<dashboard::DashboardContext>()`, no
  need to re-fetch. `dashboard.rs` holds the owner's pages, `settings.rs` the settings
  page, `shared_folder.rs` the public one.
