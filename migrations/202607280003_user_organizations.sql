CREATE FUNCTION corqty.organizations_for_current_user()
RETURNS TABLE(
    organization_id uuid,
    name text,
    slug text,
    role text
)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    SELECT organizations.id,
           organizations.name,
           organizations.slug,
           memberships.role
    FROM memberships
    JOIN organizations
      ON organizations.id = memberships.organization_id
    WHERE memberships.user_id = corqty.current_user_id()
$$;

REVOKE ALL ON FUNCTION corqty.organizations_for_current_user() FROM PUBLIC;
GRANT EXECUTE
    ON FUNCTION corqty.organizations_for_current_user()
    TO corqty_api;
