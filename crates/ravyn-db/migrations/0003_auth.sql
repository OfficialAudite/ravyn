create table sessions (
    token_hash text primary key,
    user_id uuid not null references users (id) on delete cascade,
    expires_at timestamptz not null,
    created_at timestamptz not null default now()
);

create table api_tokens (
    id uuid primary key,
    token_hash text not null unique,
    user_id uuid not null references users (id) on delete cascade,
    name text not null,
    created_at timestamptz not null default now(),
    last_used_at timestamptz
);

create index sessions_user_id_idx on sessions (user_id);
create index api_tokens_user_id_idx on api_tokens (user_id);
