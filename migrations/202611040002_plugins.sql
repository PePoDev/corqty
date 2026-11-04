CREATE TABLE plugin_versions (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    organization_id uuid NOT NULL REFERENCES organizations(id),
    key text NOT NULL CHECK (key ~ '^[a-z][a-z0-9-]{0,62}$' AND key NOT IN ('mock','shell')),
    version text NOT NULL CHECK (version ~ '^[0-9]+\.[0-9]+\.[0-9]+$' AND length(version) <= 40),
    host_contract text NOT NULL,
    manifest jsonb NOT NULL,
    checksum text NOT NULL CHECK (checksum ~ '^[0-9a-f]{64}$'),
    component bytea NOT NULL CHECK (octet_length(component) <= 26214400),
    enabled boolean NOT NULL DEFAULT true,
    installed_by uuid REFERENCES users(id),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id,key,version),
    UNIQUE (organization_id,id,checksum)
);
CREATE TABLE plugin_approvals (
    organization_id uuid NOT NULL,
    plugin_id uuid PRIMARY KEY,
    checksum text NOT NULL,
    manifest jsonb NOT NULL,
    capabilities jsonb NOT NULL,
    approved_by uuid NOT NULL REFERENCES users(id),
    approved_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (organization_id,plugin_id,checksum) REFERENCES plugin_versions(organization_id,id,checksum)
);
ALTER TABLE plugin_versions ENABLE ROW LEVEL SECURITY;
ALTER TABLE plugin_versions FORCE ROW LEVEL SECURITY;
ALTER TABLE plugin_approvals ENABLE ROW LEVEL SECURITY;
ALTER TABLE plugin_approvals FORCE ROW LEVEL SECURITY;
CREATE POLICY plugin_read ON plugin_versions FOR SELECT TO corqty_api
    USING (organization_id=corqty.current_organization_id() AND corqty.is_member(organization_id));
CREATE POLICY plugin_install ON plugin_versions FOR INSERT TO corqty_api
    WITH CHECK (organization_id=corqty.current_organization_id() AND corqty.is_admin(organization_id));
CREATE POLICY plugin_update ON plugin_versions FOR UPDATE TO corqty_api
    USING (organization_id=corqty.current_organization_id() AND corqty.is_admin(organization_id))
    WITH CHECK (organization_id=corqty.current_organization_id() AND corqty.is_admin(organization_id));
CREATE POLICY approval_read ON plugin_approvals FOR SELECT TO corqty_api
    USING (organization_id=corqty.current_organization_id() AND corqty.is_member(organization_id));
CREATE POLICY approval_insert ON plugin_approvals FOR INSERT TO corqty_api
    WITH CHECK (organization_id=corqty.current_organization_id() AND corqty.is_admin(organization_id));
CREATE POLICY approval_update ON plugin_approvals FOR UPDATE TO corqty_api
    USING (organization_id=corqty.current_organization_id() AND corqty.is_admin(organization_id))
    WITH CHECK (organization_id=corqty.current_organization_id() AND corqty.is_admin(organization_id));
GRANT SELECT,INSERT ON plugin_versions TO corqty_api;
GRANT UPDATE(enabled) ON plugin_versions TO corqty_api;
GRANT SELECT,INSERT,UPDATE ON plugin_approvals TO corqty_api;

-- Runtime compatibility belongs to the host contract, independently of which
-- org installed the plugin. Disabled versions remain runnable for existing runs.
CREATE OR REPLACE FUNCTION corqty.claim_node_runs(p_worker_id uuid,p_limit integer,p_lease_seconds integer)
RETURNS SETOF corqty.claimed_job LANGUAGE sql SECURITY DEFINER
SET search_path = public,pg_temp AS $$
    WITH worker AS (
        UPDATE worker_instances SET last_heartbeat_at=now() WHERE id=p_worker_id
        RETURNING executors,host_contracts
    ), due AS (
        SELECT n.id FROM node_runs n JOIN runs r ON r.id=n.run_id CROSS JOIN worker w
        WHERE n.state IN ('queued','retry_wait') AND n.available_at<=now()
          AND r.cancel_requested_at IS NULL
          AND (n.executor=ANY(w.executors) OR EXISTS (
              SELECT FROM plugin_versions p
              WHERE p.organization_id=n.organization_id AND p.key || '@' || p.version=n.executor
                AND p.host_contract=ANY(w.host_contracts)
          ))
        ORDER BY n.available_at,n.created_at
        LIMIT greatest(0,least(p_limit,256)) FOR UPDATE OF n SKIP LOCKED
    ) UPDATE node_runs n SET state='running',attempt=n.attempt+1,lease_token=n.lease_token+1,
        leased_by=p_worker_id,lease_expires_at=now()+make_interval(secs=>greatest(1,least(p_lease_seconds,300)))
      FROM due WHERE n.id=due.id
      RETURNING n.id,n.run_id,n.organization_id,n.executor,n.attempt,n.lease_token
$$;

CREATE FUNCTION corqty.runtime_plugin(p_worker_id uuid,p_node_run_id uuid,p_lease_token bigint)
RETURNS TABLE(manifest jsonb,component bytea,capabilities jsonb)
LANGUAGE sql SECURITY DEFINER SET search_path = public,pg_temp AS $$
    SELECT p.manifest,p.component,coalesce(a.capabilities,'[]'::jsonb)
    FROM node_runs n JOIN plugin_versions p
      ON p.organization_id=n.organization_id AND p.key || '@' || p.version=n.executor
    LEFT JOIN plugin_approvals a ON a.organization_id=p.organization_id AND a.plugin_id=p.id
      AND a.checksum=p.checksum AND a.manifest=p.manifest
    WHERE n.id=p_node_run_id AND n.leased_by=p_worker_id AND n.lease_token=p_lease_token
      AND n.state='running' AND n.lease_expires_at>now()
$$;
REVOKE ALL ON FUNCTION corqty.runtime_plugin(uuid,uuid,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.runtime_plugin(uuid,uuid,bigint) TO corqty_worker;

CREATE FUNCTION corqty.has_compatible_worker(p_org uuid,p_executor text)
RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path=public,pg_temp AS $$
    SELECT p_org=corqty.current_organization_id() AND corqty.is_member(p_org) AND EXISTS (
        SELECT FROM worker_instances w WHERE w.last_heartbeat_at>now()-interval '60 seconds'
          AND (p_executor=ANY(w.executors) OR EXISTS (
              SELECT FROM plugin_versions p WHERE p.organization_id=p_org
                AND p.key || '@' || p.version=p_executor AND p.host_contract=ANY(w.host_contracts)
          ))
    )
$$;
REVOKE ALL ON FUNCTION corqty.has_compatible_worker(uuid,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.has_compatible_worker(uuid,text) TO corqty_api;
