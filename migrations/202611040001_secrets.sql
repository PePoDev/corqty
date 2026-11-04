-- Ciphertexts are tenant scoped. The API has no SELECT grant for encrypted material:
-- metadata queries are the only reads available to a browser-facing role.
CREATE TABLE secrets (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    ciphertext bytea NOT NULL CHECK (octet_length(ciphertext) BETWEEN 17 AND 65552),
    nonce bytea NOT NULL CHECK (octet_length(nonce) = 12),
    key_version integer NOT NULL CHECK (key_version > 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT secrets_org_name UNIQUE (organization_id, name)
);
ALTER TABLE secrets ENABLE ROW LEVEL SECURITY;
ALTER TABLE secrets FORCE ROW LEVEL SECURITY;
CREATE POLICY secrets_select ON secrets FOR SELECT TO corqty_api USING (
    organization_id = corqty.current_organization_id()
    AND (SELECT corqty.is_member(corqty.current_organization_id()))
);
CREATE POLICY secrets_insert ON secrets FOR INSERT TO corqty_api WITH CHECK (
    organization_id = corqty.current_organization_id()
    AND (SELECT corqty.is_admin(corqty.current_organization_id()))
);
CREATE POLICY secrets_update ON secrets FOR UPDATE TO corqty_api USING (
    organization_id = corqty.current_organization_id()
    AND (SELECT corqty.is_admin(corqty.current_organization_id()))
) WITH CHECK (
    organization_id = corqty.current_organization_id()
    AND (SELECT corqty.is_admin(corqty.current_organization_id()))
);
CREATE POLICY secrets_delete ON secrets FOR DELETE TO corqty_api USING (
    organization_id = corqty.current_organization_id()
    AND (SELECT corqty.is_admin(corqty.current_organization_id()))
);
GRANT SELECT (id, organization_id, name, created_at, updated_at) ON secrets TO corqty_api;
GRANT INSERT ON secrets TO corqty_api;
GRANT UPDATE (name, ciphertext, nonce, key_version, updated_at) ON secrets TO corqty_api;
GRANT DELETE ON secrets TO corqty_api;

-- Serialize graph writes and deletion for each organization. A graph saved before delete
-- protects the secret; one saved after delete can remain an invalid draft but cannot publish.
CREATE FUNCTION corqty.lock_secret_references() RETURNS trigger
LANGUAGE plpgsql SET search_path = public, pg_temp AS $$
BEGIN
    PERFORM pg_advisory_xact_lock(hashtextextended(NEW.organization_id::text, 874251));
    RETURN NEW;
END
$$;
REVOKE ALL ON FUNCTION corqty.lock_secret_references() FROM PUBLIC;
CREATE TRIGGER drafts_secret_lock BEFORE INSERT OR UPDATE ON workflow_drafts
    FOR EACH ROW EXECUTE FUNCTION corqty.lock_secret_references();
CREATE TRIGGER versions_secret_lock BEFORE INSERT ON workflow_versions
    FOR EACH ROW EXECUTE FUNCTION corqty.lock_secret_references();

CREATE FUNCTION corqty.protect_secret_delete() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS $$
BEGIN
    PERFORM pg_advisory_xact_lock(hashtextextended(OLD.organization_id::text, 874251));
    IF EXISTS (
        SELECT 1 FROM workflow_drafts
        WHERE organization_id = OLD.organization_id
          AND jsonb_path_exists(graph, '$.** ? (@.kind == "secret" && @.secretId == $id)',
              jsonb_build_object('id', OLD.id::text))
        UNION ALL
        SELECT 1 FROM workflow_versions
        WHERE organization_id = OLD.organization_id
          AND jsonb_path_exists(graph, '$.** ? (@.kind == "secret" && @.secretId == $id)',
              jsonb_build_object('id', OLD.id::text))
    ) THEN
        RAISE EXCEPTION 'secret_in_use' USING ERRCODE = '23503';
    END IF;
    RETURN OLD;
END
$$;
REVOKE ALL ON FUNCTION corqty.protect_secret_delete() FROM PUBLIC;
CREATE TRIGGER secrets_delete_guard BEFORE DELETE ON secrets
    FOR EACH ROW EXECUTE FUNCTION corqty.protect_secret_delete();

-- Only an active, fenced lease can read ciphertext, and only references from its own node.
CREATE FUNCTION corqty.bound_secrets(p_worker_id uuid, p_node_run_id uuid, p_lease_token bigint)
RETURNS TABLE(id uuid, organization_id uuid, ciphertext bytea, nonce bytea, key_version integer)
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS $$
DECLARE
    v_run uuid;
    v_node jsonb;
    v_org uuid;
    v_attempt integer;
BEGIN
    v_run := corqty.lock_lease(p_worker_id, p_node_run_id, p_lease_token);
    IF v_run IS NULL THEN RETURN; END IF;
    SELECT n.organization_id, n.attempt, element INTO v_org, v_attempt, v_node
    FROM node_runs n JOIN runs r ON r.id = n.run_id
    JOIN workflow_versions w ON w.id = r.workflow_version_id
    CROSS JOIN LATERAL jsonb_array_elements(w.graph -> 'nodes') element
    WHERE n.id = p_node_run_id AND element ->> 'id' = n.node_id
      AND n.lease_expires_at > now();
    IF v_node IS NULL THEN RETURN; END IF;
    INSERT INTO audit_events(organization_id, action, resource_type, resource_id, metadata)
    SELECT v_org, 'secret.used', 'secret', s.id::text,
           jsonb_build_object('nodeRunId', p_node_run_id, 'attempt', v_attempt)
    FROM secrets s WHERE s.organization_id = v_org
      AND jsonb_path_exists(v_node, '$.** ? (@.kind == "secret" && @.secretId == $id)',
          jsonb_build_object('id', s.id::text));
    RETURN QUERY SELECT s.id, s.organization_id, s.ciphertext, s.nonce, s.key_version
    FROM secrets s WHERE s.organization_id = v_org
      AND jsonb_path_exists(v_node, '$.** ? (@.kind == "secret" && @.secretId == $id)',
          jsonb_build_object('id', s.id::text));
END
$$;
REVOKE ALL ON FUNCTION corqty.bound_secrets(uuid, uuid, bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.bound_secrets(uuid, uuid, bigint) TO corqty_worker;
