create table chunked_uploads (
    id uuid primary key,
    owner_id uuid not null references users(id) on delete cascade,
    original_name text not null,
    content_type text not null,
    total_size bigint not null,
    created_at timestamptz not null,
    expires_at timestamptz
);

create table chunked_upload_parts (
    upload_id uuid not null references chunked_uploads(id) on delete cascade,
    part_number integer not null,
    storage_key text not null,
    size_bytes bigint not null,
    primary key (upload_id, part_number)
);
