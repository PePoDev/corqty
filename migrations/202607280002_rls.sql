CREATE SCHEMA corqty;
REVOKE ALL ON SCHEMA corqty FROM PUBLIC;
GRANT USAGE ON SCHEMA corqty TO corqty_api;

CREATE FUNCTION corqty.current_user_id()
RETURNS uuid
LANGUAGE sql
STABLE
AS $$
    SELECT NULLIF(current_setting('corqty.user_id', true), '')::uuid
$$;

CREATE FUNCTION corqty.current_organization_id()
RETURNS uuid
LANGUAGE sql
STABLE
AS $$
    SELECT NULLIF(current_setting('corqty.organization_id', true), '')::uuid
$$;

CREATE FUNCTION corqty.is_member(target_organization_id uuid)
RETURNS boolean
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    SELECT EXISTS (
        SELECT 1
        FROM memberships
        WHERE organization_id = target_organization_id
          AND user_id = corqty.current_user_id()
    )
$$;

CREATE FUNCTION corqty.is_admin(target_organization_id uuid)
RETURNS boolean
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    SELECT EXISTS (
        SELECT 1
        FROM memberships
        WHERE organization_id = target_organization_id
          AND user_id = corqty.current_user_id()
          AND role = 'admin'
    )
$$;

REVOKE ALL ON FUNCTION corqty.is_member(uuid) FROM PUBLIC;
REVOKE ALL ON FUNCTION corqty.is_admin(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.is_member(uuid) TO corqty_api;
GRANT EXECUTE ON FUNCTION corqty.is_admin(uuid) TO corqty_api;

CREATE FUNCTION corqty.is_organization_creator(target_organization_id uuid)
RETURNS boolean
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    SELECT EXISTS (
        SELECT 1
        FROM organizations
        WHERE id = target_organization_id
          AND created_by = corqty.current_user_id()
    )
$$;

REVOKE ALL ON FUNCTION corqty.is_organization_creator(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.is_organization_creator(uuid) TO corqty_api;

ALTER TABLE organizations ENABLE ROW LEVEL SECURITY;
ALTER TABLE organizations FORCE ROW LEVEL SECURITY;
CREATE POLICY organizations_select ON organizations
    FOR SELECT TO corqty_api
    USING (
        id = corqty.current_organization_id()
        AND corqty.is_member(id)
    );
CREATE POLICY organizations_insert ON organizations
    FOR INSERT TO corqty_api
    WITH CHECK (
        id = corqty.current_organization_id()
        AND created_by = corqty.current_user_id()
    );
CREATE POLICY organizations_update ON organizations
    FOR UPDATE TO corqty_api
    USING (
        id = corqty.current_organization_id()
        AND corqty.is_admin(id)
    )
    WITH CHECK (
        id = corqty.current_organization_id()
        AND corqty.is_admin(id)
    );
CREATE POLICY organizations_delete ON organizations
    FOR DELETE TO corqty_api
    USING (
        id = corqty.current_organization_id()
        AND corqty.is_admin(id)
    );

ALTER TABLE memberships ENABLE ROW LEVEL SECURITY;
ALTER TABLE memberships FORCE ROW LEVEL SECURITY;
CREATE POLICY memberships_select ON memberships
    FOR SELECT TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND corqty.is_member(organization_id)
    );
CREATE POLICY memberships_insert ON memberships
    FOR INSERT TO corqty_api
    WITH CHECK (
        organization_id = corqty.current_organization_id()
        AND (
            corqty.is_admin(organization_id)
            OR (
                user_id = corqty.current_user_id()
                AND role = 'admin'
                AND corqty.is_organization_creator(organization_id)
            )
        )
    );
CREATE POLICY memberships_update ON memberships
    FOR UPDATE TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND corqty.is_admin(organization_id)
    )
    WITH CHECK (
        organization_id = corqty.current_organization_id()
        AND corqty.is_admin(organization_id)
    );
CREATE POLICY memberships_delete ON memberships
    FOR DELETE TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND corqty.is_admin(organization_id)
    );

ALTER TABLE invitations ENABLE ROW LEVEL SECURITY;
ALTER TABLE invitations FORCE ROW LEVEL SECURITY;
CREATE POLICY invitations_admin ON invitations
    FOR ALL TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND corqty.is_admin(organization_id)
    )
    WITH CHECK (
        organization_id = corqty.current_organization_id()
        AND corqty.is_admin(organization_id)
    );

ALTER TABLE audit_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE audit_events FORCE ROW LEVEL SECURITY;
CREATE POLICY audit_events_select ON audit_events
    FOR SELECT TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND corqty.is_member(organization_id)
    );
CREATE POLICY audit_events_insert ON audit_events
    FOR INSERT TO corqty_api
    WITH CHECK (
        organization_id = corqty.current_organization_id()
        AND corqty.is_member(organization_id)
    );
