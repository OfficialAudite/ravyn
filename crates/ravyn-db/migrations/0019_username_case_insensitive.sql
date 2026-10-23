-- Skipped on an instance that already has names differing only in case,
-- so the migration can't fail startup. Registration checks this anyway.
do $$
begin
    if not exists (
        select 1 from users group by lower(username) having count(*) > 1
    ) then
        create unique index users_username_lower_idx on users (lower(username));
    end if;
end
$$;
