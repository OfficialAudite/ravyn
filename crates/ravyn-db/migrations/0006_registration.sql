alter table users add column is_admin boolean not null default false;

create table instance_settings (
    id integer primary key default 1,
    registration_mode text not null default 'closed',
    check (id = 1)
);
insert into instance_settings (id, registration_mode) values (1, 'closed');

create table invites (
    id uuid primary key,
    token_hash text not null unique,
    created_by uuid not null references users(id) on delete cascade,
    created_at timestamptz not null,
    used_by uuid references users(id) on delete set null,
    used_at timestamptz
);
