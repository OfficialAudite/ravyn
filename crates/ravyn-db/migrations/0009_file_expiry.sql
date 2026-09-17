alter table files add column expires_at timestamptz;
alter table instance_settings add column default_expiry_preset text not null default 'never';
