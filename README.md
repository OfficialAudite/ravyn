# ravyn

A self-hosted file hosting service, built entirely in Rust for performance and low
operating overhead. Alternative to [chibisafe](https://github.com/chibisafe/chibisafe)
and [zipline](https://github.com/diced/zipline).

## Architecture

```text
crates/
  ravyn-core/     domain types (File, ids, errors) — no framework dependencies
  ravyn-storage/  object storage (S3-compatible or local disk) via `object_store`
  ravyn-db/       Postgres access and migrations via `sqlx`
  ravyn-api/      HTTP API (axum): auth, uploads, file serving
  ravyn-web/      frontend (Leptos, server-rendered + hydrated)
```

Each crate is independent and only depends on the ones below it in the list. This is
deliberate: you should be able to fork this repo and rip out or replace a whole layer
(swap `ravyn-web` for a different frontend, swap `ravyn-storage`'s backend, etc.)
without touching the rest.

## Running locally

You need Postgres reachable via `DATABASE_URL`.

```sh
# API server (auth, uploads, file serving)
DATABASE_URL=postgres://localhost/ravyn STORAGE_ROOT=./data cargo run -p ravyn-api

# Web frontend (separate process, talks to the API server-side)
RAVYN_API_URL=http://127.0.0.1:3000 cargo leptos watch --project ravyn-web
```

The web UI never calls the API from the browser directly (no CORS setup needed): its
Leptos server functions run server-side, forward the session cookie to `ravyn-api` by
hand, and relay any `Set-Cookie` back. See `crates/ravyn-web/src/server_fns.rs`. File
uploads from the browser go through a plain HTML form (`POST /upload` on `ravyn-web`,
which proxies to the API) rather than a server function, so they keep working without
JS.

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

## Extending it

- **New storage backend**: add a variant to `StorageConfig` in
  `crates/ravyn-storage/src/config.rs`. `object_store` already has builders for GCS,
  Azure, and local disk in addition to S3.
- **New API route**: add a handler in `crates/ravyn-api/src/routes.rs` and register it
  in `router()`.
- **New DB query**: add it to `crates/ravyn-db/src/files.rs` (or a new module,
  following the same pattern) as a method on `Db`.
- **Schema change**: add a new file under `crates/ravyn-db/migrations/`, run via
  `Db::migrate`.
- **New web page/action**: add a `#[server]` function in
  `crates/ravyn-web/src/server_fns.rs` (it forwards to the API and relays cookies for
  you) and a component in `crates/ravyn-web/src/app.rs`.
