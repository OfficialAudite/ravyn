create table users (
    id uuid primary key,
    username text not null unique,
    password_hash text not null,
    created_at timestamptz not null default now()
);
