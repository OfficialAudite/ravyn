create table short_urls (
    id uuid primary key,
    owner_id uuid not null references users(id) on delete cascade,
    slug text not null unique,
    destination text not null,
    clicks bigint not null default 0,
    created_at timestamptz not null
);

create index short_urls_owner_idx on short_urls(owner_id);
