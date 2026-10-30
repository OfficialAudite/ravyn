-- Set when the backend takes parts directly (S3): each chunk goes straight
-- into one multipart upload instead of its own temporary object.
alter table chunked_uploads add column storage_key text, add column multipart_id text;

alter table chunked_upload_parts alter column storage_key drop not null;
alter table chunked_upload_parts add column part_id text;
