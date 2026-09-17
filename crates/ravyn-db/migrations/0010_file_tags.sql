alter table files add column tags text[] not null default '{}';
create index files_tags_idx on files using gin (tags);
