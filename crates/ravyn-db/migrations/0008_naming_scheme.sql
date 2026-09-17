alter table instance_settings add column naming_scheme text not null default 'original';
alter table instance_settings add column random_name_length integer not null default 8;
