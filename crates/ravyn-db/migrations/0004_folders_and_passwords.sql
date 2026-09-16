create table folders (
    id uuid primary key,
    owner_id uuid not null references users (id) on delete cascade,
    name text not null,
    password_hash text,
    created_at timestamptz not null default now()
);

create index folders_owner_id_idx on folders (owner_id);

alter table files add column folder_id uuid references folders (id) on delete set null;
alter table files add column password_hash text;

create index files_folder_id_idx on files (folder_id);
