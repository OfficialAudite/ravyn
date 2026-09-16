create table files (
    id uuid primary key,
    owner_id uuid not null references users (id) on delete cascade,
    original_name text not null,
    storage_key text not null unique,
    content_type text not null,
    size_bytes bigint not null,
    sha256 text not null,
    created_at timestamptz not null default now()
);

create index files_owner_id_idx on files (owner_id);
create index files_sha256_idx on files (sha256);
