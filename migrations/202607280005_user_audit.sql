CREATE FUNCTION corqty.audit_current_user_memberships(
    audit_action text,
    audit_resource_type text,
    audit_resource_id text,
    audit_metadata jsonb
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
DECLARE
    actor_id uuid;
BEGIN
    actor_id := corqty.current_user_id();
    IF actor_id IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'missing_user_context';
    END IF;

    INSERT INTO audit_events(
        organization_id,
        actor_user_id,
        action,
        resource_type,
        resource_id,
        metadata
    )
    SELECT memberships.organization_id,
           actor_id,
           audit_action,
           audit_resource_type,
           audit_resource_id,
           audit_metadata
    FROM memberships
    WHERE memberships.user_id = actor_id;
END;
$$;

REVOKE ALL
    ON FUNCTION corqty.audit_current_user_memberships(
        text, text, text, jsonb
    )
    FROM PUBLIC;
GRANT EXECUTE
    ON FUNCTION corqty.audit_current_user_memberships(
        text, text, text, jsonb
    )
    TO corqty_api;
