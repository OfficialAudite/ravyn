create table activity_log (
    id uuid primary key,
    user_id uuid references users(id) on delete set null,
    username text not null,
    action text not null,
    target text,
    created_at timestamptz not null
);

create index activity_log_created_idx on activity_log (created_at desc);
