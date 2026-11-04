CREATE TABLE service_tokens (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    organization_id uuid NOT NULL REFERENCES organizations(id),
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    scope text NOT NULL DEFAULT 'workflow:run' CHECK (scope = 'workflow:run'),
    token_hash bytea NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    created_by uuid REFERENCES users(id),
    created_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz
);
ALTER TABLE service_tokens ENABLE ROW LEVEL SECURITY;
ALTER TABLE service_tokens FORCE ROW LEVEL SECURITY;
CREATE POLICY service_tokens_admin ON service_tokens TO corqty_api
    USING (organization_id = corqty.current_organization_id() AND corqty.is_admin(organization_id))
    WITH CHECK (organization_id = corqty.current_organization_id() AND corqty.is_admin(organization_id));
GRANT SELECT, INSERT, UPDATE ON service_tokens TO corqty_api;

CREATE TABLE triggers (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    organization_id uuid NOT NULL REFERENCES organizations(id),
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    workflow_id uuid NOT NULL,
    workflow_version_id uuid NOT NULL,
    version integer NOT NULL CHECK (version > 0),
    kind text NOT NULL CHECK (kind IN ('cron', 'webhook')),
    input jsonb NOT NULL DEFAULT '{}',
    enabled boolean NOT NULL DEFAULT true,
    cron text,
    timezone text,
    overlap_policy text NOT NULL DEFAULT 'skip' CHECK (overlap_policy IN ('skip', 'allow')),
    next_scheduled_at timestamptz,
    webhook_ciphertext bytea,
    webhook_nonce bytea,
    webhook_key_version integer,
    created_by uuid REFERENCES users(id),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (organization_id, workflow_id) REFERENCES workflows(organization_id, id),
    FOREIGN KEY (organization_id, workflow_version_id) REFERENCES workflow_versions(organization_id, id),
    CHECK ((kind = 'cron' AND cron IS NOT NULL AND timezone IS NOT NULL AND next_scheduled_at IS NOT NULL
            AND webhook_ciphertext IS NULL AND webhook_nonce IS NULL)
        OR (kind = 'webhook' AND cron IS NULL AND timezone IS NULL AND next_scheduled_at IS NULL
            AND webhook_ciphertext IS NOT NULL AND octet_length(webhook_nonce) = 12 AND webhook_key_version = 1))
);
CREATE INDEX triggers_due ON triggers(next_scheduled_at) WHERE enabled AND kind = 'cron';
ALTER TABLE triggers ENABLE ROW LEVEL SECURITY;
ALTER TABLE triggers FORCE ROW LEVEL SECURITY;
CREATE POLICY triggers_select ON triggers FOR SELECT TO corqty_api
    USING (organization_id = corqty.current_organization_id() AND corqty.is_member(organization_id));
CREATE POLICY triggers_insert ON triggers FOR INSERT TO corqty_api
    WITH CHECK (organization_id = corqty.current_organization_id() AND corqty.can_edit(organization_id));
CREATE POLICY triggers_update ON triggers FOR UPDATE TO corqty_api
    USING (organization_id = corqty.current_organization_id() AND corqty.can_edit(organization_id))
    WITH CHECK (organization_id = corqty.current_organization_id() AND corqty.can_edit(organization_id));
CREATE POLICY triggers_delete ON triggers FOR DELETE TO corqty_api
    USING (organization_id = corqty.current_organization_id() AND corqty.can_edit(organization_id));
GRANT SELECT, INSERT, UPDATE, DELETE ON triggers TO corqty_api;

ALTER TABLE runs DROP CONSTRAINT runs_trigger_source_check;
ALTER TABLE runs ADD CONSTRAINT runs_trigger_source_check CHECK (trigger_source IN ('manual', 'api', 'cron', 'webhook'));
ALTER TABLE runs ADD COLUMN service_token_id uuid REFERENCES service_tokens(id);
ALTER TABLE runs ADD COLUMN trigger_id uuid REFERENCES triggers(id) ON DELETE SET NULL;

CREATE TABLE trigger_occurrences (
    trigger_id uuid NOT NULL REFERENCES triggers(id) ON DELETE CASCADE,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    scheduled_for timestamptz NOT NULL,
    run_id uuid REFERENCES runs(id) ON DELETE SET NULL,
    outcome text NOT NULL CHECK (outcome IN ('started', 'overlap_skipped', 'misfire_skipped', 'rejected')),
    PRIMARY KEY (trigger_id, scheduled_for)
);
ALTER TABLE trigger_occurrences ENABLE ROW LEVEL SECURITY;
ALTER TABLE trigger_occurrences FORCE ROW LEVEL SECURITY;
CREATE POLICY trigger_occurrences_tenant ON trigger_occurrences TO corqty_api
    USING (organization_id = corqty.current_organization_id() AND corqty.is_member(organization_id))
    WITH CHECK (organization_id = corqty.current_organization_id() AND corqty.can_run(organization_id));
GRANT SELECT, INSERT ON trigger_occurrences TO corqty_api;

CREATE TABLE run_idempotency (
    organization_id uuid NOT NULL REFERENCES organizations(id),
    route text NOT NULL,
    actor text NOT NULL,
    key text NOT NULL CHECK (octet_length(key) BETWEEN 1 AND 200),
    request_hash bytea NOT NULL,
    run_id uuid NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    expires_at timestamptz NOT NULL DEFAULT now() + interval '24 hours',
    PRIMARY KEY (organization_id, route, actor, key)
);
ALTER TABLE run_idempotency ENABLE ROW LEVEL SECURITY;
ALTER TABLE run_idempotency FORCE ROW LEVEL SECURITY;
CREATE POLICY run_idempotency_tenant ON run_idempotency TO corqty_api
    USING (organization_id = corqty.current_organization_id() AND corqty.can_run(organization_id))
    WITH CHECK (organization_id = corqty.current_organization_id() AND corqty.can_run(organization_id));
GRANT SELECT, INSERT, DELETE ON run_idempotency TO corqty_api;

-- Context grants only read/run rights to authenticated, non-user actors. Admin/editor
-- checks still require a real membership, and token middleware exposes only POST runs.
CREATE FUNCTION corqty.automation_actor(target uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp AS $$
    SELECT EXISTS (SELECT FROM service_tokens WHERE organization_id = target AND revoked_at IS NULL
        AND id = NULLIF(current_setting('corqty.service_token_id', true), '')::uuid)
    OR EXISTS (SELECT FROM triggers WHERE organization_id = target AND enabled
        AND id = NULLIF(current_setting('corqty.trigger_id', true), '')::uuid)
$$;
REVOKE ALL ON FUNCTION corqty.automation_actor(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.automation_actor(uuid) TO corqty_api;
CREATE OR REPLACE FUNCTION corqty.is_member(target_organization_id uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp AS $$
    SELECT EXISTS (SELECT FROM memberships WHERE organization_id = target_organization_id
        AND user_id = corqty.current_user_id()) OR corqty.automation_actor(target_organization_id)
$$;
CREATE OR REPLACE FUNCTION corqty.can_run(target_organization_id uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp AS $$
    SELECT EXISTS (SELECT FROM memberships WHERE organization_id = target_organization_id
        AND user_id = corqty.current_user_id() AND role IN ('admin', 'editor', 'runner'))
        OR corqty.automation_actor(target_organization_id)
$$;

CREATE FUNCTION corqty.authenticate_service_token(p_hash bytea)
RETURNS TABLE(id uuid, organization_id uuid)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp AS $$
    SELECT id, organization_id FROM service_tokens WHERE token_hash = p_hash AND revoked_at IS NULL
$$;
REVOKE ALL ON FUNCTION corqty.authenticate_service_token(bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.authenticate_service_token(bytea) TO corqty_api;

CREATE FUNCTION corqty.webhook_trigger(p_id uuid) RETURNS SETOF triggers
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp AS $$
    SELECT * FROM triggers WHERE id = p_id AND kind = 'webhook' AND enabled
$$;
REVOKE ALL ON FUNCTION corqty.webhook_trigger(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.webhook_trigger(uuid) TO corqty_api;

CREATE FUNCTION corqty.lock_automation_trigger(p_id uuid) RETURNS SETOF triggers
LANGUAGE sql SECURITY DEFINER SET search_path = public, pg_temp AS $$
    SELECT * FROM triggers WHERE id = p_id AND enabled
        AND id = NULLIF(current_setting('corqty.trigger_id', true), '')::uuid
        AND organization_id = corqty.current_organization_id() FOR UPDATE
$$;
REVOKE ALL ON FUNCTION corqty.lock_automation_trigger(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.lock_automation_trigger(uuid) TO corqty_api;

-- Advisory leader election and row claims share the dispatch transaction. A crash rolls
-- both the occurrence and next timestamp back; a second API can safely pick it up.
CREATE FUNCTION corqty.due_triggers(p_now timestamptz) RETURNS SETOF triggers
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS $$
BEGIN
    IF NOT pg_try_advisory_xact_lock(1129271889, 2) THEN RETURN; END IF;
    RETURN QUERY SELECT * FROM triggers WHERE enabled AND kind = 'cron' AND next_scheduled_at <= p_now
        ORDER BY next_scheduled_at, id LIMIT 100 FOR UPDATE SKIP LOCKED;
END
$$;
REVOKE ALL ON FUNCTION corqty.due_triggers(timestamptz) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.due_triggers(timestamptz) TO corqty_api;
CREATE FUNCTION corqty.advance_trigger(p_id uuid, p_next timestamptz) RETURNS void
LANGUAGE sql SECURITY DEFINER SET search_path = public, pg_temp AS $$
    UPDATE triggers SET next_scheduled_at = p_next WHERE id = p_id
        AND id = NULLIF(current_setting('corqty.trigger_id', true), '')::uuid
        AND organization_id = corqty.current_organization_id() AND kind = 'cron'
$$;
REVOKE ALL ON FUNCTION corqty.advance_trigger(uuid,timestamptz) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.advance_trigger(uuid,timestamptz) TO corqty_api;

CREATE TABLE rate_limits (
    key bytea PRIMARY KEY,
    window_start timestamptz NOT NULL,
    attempts integer NOT NULL
);
ALTER TABLE rate_limits ENABLE ROW LEVEL SECURITY;
ALTER TABLE rate_limits FORCE ROW LEVEL SECURITY;
CREATE POLICY rate_limits_none ON rate_limits TO corqty_api USING (false);
CREATE FUNCTION corqty.rate_limit(p_key bytea, p_limit integer, p_seconds integer) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS $$
DECLARE v_attempts integer;
BEGIN
    IF p_limit NOT BETWEEN 1 AND 100000 OR p_seconds NOT BETWEEN 1 AND 3600 THEN RETURN false; END IF;
    INSERT INTO rate_limits(key, window_start, attempts) VALUES (p_key, clock_timestamp(), 1)
    ON CONFLICT (key) DO UPDATE SET
        attempts = CASE WHEN rate_limits.window_start <= clock_timestamp() - make_interval(secs => p_seconds)
                        THEN 1 ELSE least(rate_limits.attempts + 1, 100001) END,
        window_start = CASE WHEN rate_limits.window_start <= clock_timestamp() - make_interval(secs => p_seconds)
                           THEN clock_timestamp() ELSE rate_limits.window_start END
    RETURNING attempts INTO v_attempts;
    RETURN v_attempts <= p_limit;
END
$$;
REVOKE ALL ON FUNCTION corqty.rate_limit(bytea,integer,integer) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.rate_limit(bytea,integer,integer) TO corqty_api;

CREATE FUNCTION corqty.retain_runs() RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp AS $$
BEGIN
    IF NOT pg_try_advisory_xact_lock(1129271889, 3) THEN RETURN; END IF;
    DELETE FROM run_events WHERE created_at < now() - interval '30 days';
    DELETE FROM runs WHERE finished_at < now() - interval '90 days' AND status IN ('succeeded','failed','canceled');
    DELETE FROM run_idempotency WHERE expires_at <= now();
    DELETE FROM rate_limits WHERE window_start < now() - interval '1 day';
END
$$;
REVOKE ALL ON FUNCTION corqty.retain_runs() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.retain_runs() TO corqty_api;
