alter table instance_settings add column cost_per_gb_month double precision;
alter table instance_settings add column cost_currency text not null default '$';
