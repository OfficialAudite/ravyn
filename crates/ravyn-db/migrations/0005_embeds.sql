alter table users add column embed_enabled boolean not null default false;
alter table users add column embed_title text;
alter table users add column embed_description text;
alter table users add column embed_color text;
alter table users add column embed_site_name text;
