alter table users add column totp_secret text;
alter table users add column totp_enabled boolean not null default false;
alter table users add column totp_recovery_codes text[] not null default '{}';

create table pending_logins (
    token_hash text primary key,
    user_id uuid not null references users(id) on delete cascade,
    created_at timestamptz not null,
    expires_at timestamptz not null
);
