CREATE FUNCTION corqty.assert_organization_has_admin(
    affected_organization_id uuid
)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
BEGIN
    PERFORM 1
    FROM organizations
    WHERE id = affected_organization_id
    FOR UPDATE;

    IF NOT FOUND THEN
        RETURN;
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM memberships
        WHERE organization_id = affected_organization_id
          AND role = 'admin'
    ) THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'final_admin_required';
    END IF;
END;
$$;

REVOKE ALL
    ON FUNCTION corqty.assert_organization_has_admin(uuid)
    FROM PUBLIC;

CREATE FUNCTION corqty.ensure_organization_has_admin()
RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        PERFORM corqty.assert_organization_has_admin(NEW.organization_id);
    ELSIF TG_OP = 'DELETE' THEN
        PERFORM corqty.assert_organization_has_admin(OLD.organization_id);
    ELSE
        PERFORM corqty.assert_organization_has_admin(OLD.organization_id);
        IF NEW.organization_id <> OLD.organization_id THEN
            PERFORM corqty.assert_organization_has_admin(NEW.organization_id);
        END IF;
    END IF;
    RETURN NULL;
END;
$$;

REVOKE ALL
    ON FUNCTION corqty.ensure_organization_has_admin()
    FROM PUBLIC;

CREATE CONSTRAINT TRIGGER memberships_require_admin
AFTER INSERT OR UPDATE OR DELETE ON memberships
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW
EXECUTE FUNCTION corqty.ensure_organization_has_admin();

CREATE FUNCTION corqty.accept_invitation(
    supplied_token_hash bytea
)
RETURNS TABLE(organization_id uuid, accepted_role text)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
DECLARE
    invitation_record invitations%ROWTYPE;
    accepting_user_id uuid;
    accepting_email text;
BEGIN
    accepting_user_id := corqty.current_user_id();
    IF accepting_user_id IS NULL THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'invalid_invitation';
    END IF;

    SELECT email INTO accepting_email
    FROM users
    WHERE id = accepting_user_id;

    SELECT * INTO invitation_record
    FROM invitations
    WHERE token_hash = supplied_token_hash
      AND accepted_at IS NULL
      AND expires_at > now()
    FOR UPDATE;

    IF NOT FOUND OR invitation_record.email <> accepting_email THEN
        RAISE EXCEPTION USING
            ERRCODE = 'P0001',
            MESSAGE = 'invalid_invitation';
    END IF;

    INSERT INTO memberships(organization_id, user_id, role)
    VALUES (
        invitation_record.organization_id,
        accepting_user_id,
        invitation_record.role
    )
    ON CONFLICT ON CONSTRAINT memberships_pkey
    DO UPDATE SET role = EXCLUDED.role;

    UPDATE invitations
    SET accepted_at = now()
    WHERE id = invitation_record.id;

    INSERT INTO audit_events(
        organization_id,
        actor_user_id,
        action,
        resource_type,
        resource_id,
        metadata
    )
    VALUES (
        invitation_record.organization_id,
        accepting_user_id,
        'invitation.accepted',
        'membership',
        accepting_user_id::text,
        jsonb_build_object('role', invitation_record.role)
    );

    RETURN QUERY
    SELECT invitation_record.organization_id, invitation_record.role;
END;
$$;

REVOKE ALL ON FUNCTION corqty.accept_invitation(bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.accept_invitation(bytea) TO corqty_api;
