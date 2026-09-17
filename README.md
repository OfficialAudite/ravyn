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
JS. Thumbnails (`GET /preview/{id}`) and the file detail modal's inline preview/
download/"open original" (`GET /raw/{id}`) are proxied the same way, on `ravyn-web`,
so your own password-protected files still show a preview in your own dashboard — a
plain `<img>`/`<video>` pointed straight at `ravyn-api` would be a cross-origin
request that never carries the `ravyn-web` session cookie. `/raw/{id}` streams the
response through rather than buffering it like the (small, AVIF) thumbnails, since an
original file can be arbitrarily large.

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

That's the S3-side chunking; separately, both `ravyn-api` and `ravyn-web` cap the
size of an incoming upload request itself (Axum's own default is 2 MB, nowhere near
enough for real files) — `MAX_UPLOAD_MB` overrides it on both, in megabytes, and
needs to be set the same on both since `ravyn-web`'s `/upload` receives the whole
request itself before proxying it on to `ravyn-api`. Each part of a multipart upload
is still buffered fully in memory before being written to storage, so this is a
generous default (2048 MB), not an unbounded one.

Dropping (or selecting) more than one file at once at `/upload` sends them all as
separate parts of the same request — `POST /files` accepts one or many parts. A
single-part request (what ShareX always sends) gets the original `{"id": ...}`
response so existing ShareX configs (`{json:id}`) don't break; more than one part
gets a `[{"name","id","error"}]` array instead, one entry per file, since a later
file hitting the owner's storage quota shouldn't discard the ones already saved
before it.

Thumbnails (see below) are always local disk, under `THUMBNAIL_ROOT`, regardless of
where the original files live — they're small, read on every gallery view, and there's
no reason to round-trip them through S3.

Serving is always a passthrough, never a redirect: `GET /files/{id}` reads the bytes
from wherever they live and streams them back from `ravyn-api` itself. Point your own
domain at `ravyn-api` and every link — thumbnails, downloads, shared folders — stays on
that domain no matter which backend is storing the bytes.

## Auth

The first account created — via the web UI at `/register`, or via the CLI below —
automatically becomes the instance's admin. Everyone after that is gated by whatever
registration mode the admin sets from `/admin` (`RegistrationMode` in `ravyn-core`):

- **closed** (default) — no self-service registration; accounts only via the CLI or
  an admin-issued invite.
- **open** — anyone who can reach the server can create an account.
- **invite** — registering requires a single-use invite code, generated from
  `/admin` by an admin.

```sh
DATABASE_URL=postgres://localhost/ravyn cargo run -p ravyn-api -- create-user alice hunter2
```

The CLI always creates an admin (shell access already implies that level of trust)
and works regardless of the current registration mode — handy for headless setups or
adding another admin later.

- `POST /register` (`{"username", "password", "invite_token"}`) creates an account,
  subject to the current registration mode, and signs you in — same cookie as login.
- `GET /registration-status` (public) reports whether the instance still needs its
  first account and what the current registration mode is, so the login screen knows
  whether to offer a "create an account" link.
- `POST /login` (`{"username", "password"}`) sets a session cookie, for the web UI.
- `POST /api-tokens` (`{"name"}`, requires a session cookie) mints an API token,
  returned once as `{"token"}`. This is what goes in a ShareX config.
- `POST /files` accepts either the session cookie or an `Authorization: Bearer
  <token>` header — ShareX only ever uses the latter.

Creating a token from the web UI (`/settings`) offers a **download ShareX config**
button right there — a `.sxcu` with your token and this instance's URL already filled
in, ready to import. [`contrib/sharex/ravyn.sxcu`](contrib/sharex/ravyn.sxcu) is kept
as a plain reference for anyone configuring ShareX by hand instead (e.g. scripting a
headless setup) — replace `YOUR_API_TOKEN` there yourself.

### Per-user storage quotas

`/admin` shows every account on the instance with its current storage usage, and a
field to set (or clear) a per-user cap in MB (`users.max_storage_bytes`,
`NULL` = unlimited). `POST /files` checks the owner's cap against their current usage
— summed fresh from `files.size_bytes` on every upload rather than kept as a running
counter, since a self-hosted instance's file count stays small enough that this is
cheap and it can never drift — and rejects with `507 Insufficient Storage` if the
upload would exceed it.

### Stats

Every user sees their own usage in `/settings` (`GET /me/stats`): storage used
against their quota (or "unlimited"), total files, and a breakdown by type
(images/videos/audio/documents/other). An admin additionally sees the same shape
summed across the whole instance in `/admin` (`GET /admin/stats`) — total users,
total files, total storage, and the instance-wide type breakdown. Both reuse the
existing per-owner file list rather than a new aggregate query or any kind of
stored counter, for the same reason the quota check does.

## Embeds (Discord, Slack, Twitter)

Every share link already works as a direct image/video link — paste one in Discord
and it previews natively, embeds on or off. Turning embeds on (`/settings`, or
`PUT /embed-settings`) additionally sets a custom title, description, and accent color
that Discord/Slack/Twitter read from the link, the same feature under the same name in
Zipline.

This works by giving every file a stable **view link** (`GET /v/{id}`, what copy-link
and the ShareX config actually point at) instead of the raw `/files/{id}` URL:

- Embeds off (the default): `/v/{id}` just 302-redirects to the raw file. Discord
  follows the redirect and previews the raw image/video exactly as it would if you'd
  linked it directly — nothing about today's behavior changes.
- Embeds on: `/v/{id}` renders an actual HTML page with Open Graph tags (`og:title`,
  `og:description`, `og:image`/`og:video`/`og:audio` depending on content type,
  `theme-color` for Discord's accent stripe) pointing back at the raw file, plus a
  visible `<img>`/`<video>`/`<audio>` tag so a human opening the link directly still
  sees the file, not a bare meta-tag page.

`embedTitle`/`embedDescription`/`embedSiteName` support `{file.name}`, `{file.size}`,
`{file.type}`, and `{user.username}` template variables, substituted in
`crates/ravyn-api/src/routes/view.rs`. Settings are per-user (`users.embed_*` columns),
not global — matches Zipline's model, and fits naturally since ravyn already scopes
everything else per-owner.

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

The logged-in app is four pages under a shared layout (`DashboardLayout` in
`crates/ravyn-web/src/dashboard.rs`, using nested Leptos routes with an `<Outlet/>`):

- `/` — browse: folder sidebar, search/type/sort filters, the file grid.
- `/upload` — just the dropzone.
- `/settings` — tabbed: **general** (account info + your own storage/file stats),
  **api tokens**, **embeds**, **storage** (read-only backend summary).
- `/admin` — instance-wide config, visible only to an admin (`crates/ravyn-web/src/admin.rs`,
  a "admin" nav link appears automatically for one). Also tabbed: **general**
  (instance-wide stats + registration mode + invite codes), **users** (every account,
  their usage, and their storage limit).

All four share one set of file/folder resources and mutation actions via
`provide_context`/`expect_context`, so an action taken on one page (say, deleting a
file) is reflected everywhere without a separate fetch per page. `/f/{id}` (the public
shared-folder view) stays a sibling top-level route, outside this layout, since it's
usable without logging in at all.

Multiple users are supported — every file/folder is scoped to its owner, and an
admin controls from `/admin` whether anyone else can register (see [Auth](#auth)).
There's no shared/team view of another user's files; each account's hoard is its own.

## Docker

```sh
docker compose up -d --build
```

Then either visit `/register` in a browser to create the first (admin) account, or
run `docker compose run --rm api create-user alice hunter2` for a headless setup.

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
