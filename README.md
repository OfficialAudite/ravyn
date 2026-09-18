# ravyn

A self-hosted file hosting service, built entirely in Rust for performance and low
operating overhead. An alternative to [chibisafe](https://github.com/chibisafe/chibisafe)
and [zipline](https://github.com/diced/zipline).

**[tryravyn.com](https://tryravyn.com)** has install guides (Docker and bare metal)
and a full feature reference. This README stays close to the code, for anyone
building or extending ravyn itself.

## Architecture

```text
crates/
  ravyn-core/     domain types (File, Folder, ids, errors), no framework dependencies
  ravyn-storage/  object storage (S3-compatible or local disk) via `object_store`
  ravyn-db/       Postgres access and migrations via `sqlx`
  ravyn-api/      HTTP API (axum): auth, uploads, folders, file serving
  ravyn-web/      frontend (Leptos, server-rendered + hydrated)
```

Each crate is independent and only depends on the ones below it in the list. This is
deliberate: you should be able to fork this repo and rip out or replace a whole layer
(swap `ravyn-web` for a different frontend, swap `ravyn-storage`'s backend, etc.)
without touching the rest.

The logged-in web UI is four pages under a shared layout
(`DashboardLayout` in `crates/ravyn-web/src/dashboard.rs`): `/` (browse),
`/upload`, `/settings` (per-account), and `/admin` (instance-wide, admin only).
`/f/{id}` (a shared folder) is a public route outside that layout, usable without
logging in.

- [Running locally](#running-locally)
- [Storage backends](#storage-backends)
- [Auth](#auth)
- [Docker](#docker)
- [Extending it](#extending-it)

## Running locally

You need Postgres reachable via `DATABASE_URL`.

```sh
# API server (auth, uploads, folders, file serving)
DATABASE_URL=postgres://localhost/ravyn STORAGE_ROOT=./data cargo run -p ravyn-api

# Web frontend (separate process, talks to the API server-side)
RAVYN_API_URL=http://127.0.0.1:3000 cargo leptos watch --project ravyn-web
```

Or via `docker compose up`, see [Docker](#docker) below.

The web UI never calls the API from the browser directly (no CORS setup needed): its
Leptos server functions run server-side, forward the session cookie to `ravyn-api` by
hand, and relay any `Set-Cookie` back. See `crates/ravyn-web/src/server_fns/`. File
uploads from the browser go through a plain HTML form (`POST /upload` on `ravyn-web`,
which proxies to the API) rather than a server function, so they keep working without
JS. Thumbnails and the file preview/download routes are proxied the same way, on
`ravyn-web`, so your own password-protected files still show a preview in your own
dashboard: a plain `<img>`/`<video>` pointed straight at `ravyn-api` would be a
cross-origin request that never carries the `ravyn-web` session cookie.

## Storage backends

Set `STORAGE_BACKEND=s3` to store uploaded files in S3 (or anything S3-compatible,
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

Leave `STORAGE_BACKEND` unset to store on local disk under `STORAGE_ROOT` instead.
Either way, uploads go through `object_store`'s multipart API
(`crates/ravyn-storage/src/lib.rs`) and stream a chunk at a time rather than
buffering a whole file in memory, verified by watching the API container's memory
stay flat while uploading a 200MB file. Thumbnails are always local disk, under
`THUMBNAIL_ROOT`, regardless of where originals live. `MAX_UPLOAD_MB` caps an
incoming request's size on both `ravyn-api` and `ravyn-web` (Axum's own default,
2MB, is nowhere near enough for real files) and needs to match on both, since
`ravyn-web`'s `/upload` receives the whole request before proxying it on.

Serving is always a passthrough, never a redirect: `GET /files/{id}` reads the bytes
from wherever they live and streams them back from `ravyn-api` itself, so every link
stays on your domain no matter which backend is storing the bytes.

## Auth

The first account created, via the web UI at `/register` or the CLI below,
automatically becomes the instance's admin. Everyone after that is gated by
whatever registration mode the admin sets from `/admin` (`RegistrationMode` in
`ravyn-core`): **closed** (default), **open**, or **invite** (a single-use code an
admin generates).

```sh
DATABASE_URL=postgres://localhost/ravyn cargo run -p ravyn-api -- create-user alice hunter2
```

That command always creates an admin, regardless of the current registration mode,
useful for a headless setup or adding another admin later.

- `POST /register` creates an account, subject to the current registration mode.
- `POST /login` sets a session cookie, for the web UI.
- `POST /api-tokens` mints an API token, returned once, for ShareX or a script:
  `POST /files` accepts either the session cookie or an
  `Authorization: Bearer <token>` header.

The settings page has a one-click "download ShareX config" button that fills in a
fresh token and this instance's URL. [`contrib/sharex/ravyn.sxcu`](contrib/sharex/ravyn.sxcu)
is a plain reference for configuring ShareX by hand instead.

2FA, per-user quotas, rate limiting, and everything else auth-related is on
[tryravyn.com's features page](https://tryravyn.com/docs/features.html).

## Docker

```sh
docker compose up -d --build
```

Then either visit `/register` in a browser to create the first (admin) account, or
run `docker compose run --rm api create-user alice hunter2` for a headless setup.

Serves the API on `:3000` and the web UI on `:3001`. See `docker-compose.yml` for the
Postgres, storage-volume, and `RAVYN_PUBLIC_API_URL` wiring, that one matters on
*both* services, for two different reasons: `ravyn-web` reaches `ravyn-api` over the
compose network as `http://api:3000`, but the *browser* needs the host-mapped
`http://localhost:3000` instead for thumbnails and download links, since it can't
resolve the `api` hostname; `ravyn-api` needs its own copy for any link it builds
itself from a request that came in through `ravyn-web`'s `/upload` proxy (a webhook
notification, currently), since that request's own headers carry `ravyn-web`'s
address, not the browser's.

**Deploying behind a real domain**: set both to that domain (`https://your.domain`,
not `localhost`), this is the single most common misconfiguration after a first
deploy.

**Prebuilt images**: `.github/workflows/docker-publish.yml` builds and pushes
`ghcr.io/officialaudite/ravyn-api` and `ghcr.io/officialaudite/ravyn-web` on every
push to `master` (tagged `latest`) and on any `v*` git tag (tagged with that
version). Swap `build:` for `image: ghcr.io/officialaudite/ravyn-api:latest` (and
the same for `web`) in `docker-compose.yml` to run from those instead of building
locally. A package is private by default the first time it's published, even on a
public repo, go to the package's own settings on GitHub and make it public, or the
`image:` pull above fails for anyone without registry access.

See [tryravyn.com/docs](https://tryravyn.com/docs/) for a from-scratch install guide,
including a bare-metal path (Postgres + systemd + Caddy, no Docker) for anyone who'd
rather not run containers.

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
  you). For a new *page*, add a component and a nested `<Route>` in `app.rs`, reach the
  shared file/folder state via `expect_context::<dashboard::DashboardContext>()`, no
  need to re-fetch. `dashboard.rs` holds the browse/upload pages, `settings.rs` the
  per-user settings page, `admin.rs` the admin-only page, `register.rs` and
  `shared_folder.rs` the two public (unauthenticated) pages.
- **New settings/admin tab**: both `/settings` and `/admin` are a `RwSignal`-backed
  tab enum plus a `match` over it (`SettingsTab` in `settings.rs`, `AdminTab` in
  `admin.rs`), add a variant, a `.settings-tab` button, and a match arm rendering
  whatever section component you add.

## License

[MIT](LICENSE)
