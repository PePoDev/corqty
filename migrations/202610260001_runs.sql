CREATE FUNCTION corqty.can_run(target_organization_id uuid)
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
          AND role IN ('admin', 'editor', 'runner')
    )
$$;

REVOKE ALL ON FUNCTION corqty.can_run(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.can_run(uuid) TO corqty_api;

-- Workers reach run data only through definer functions in this schema, each granted to
-- corqty_worker on its own. The role gets no table privileges.
GRANT USAGE ON SCHEMA corqty TO corqty_worker;

-- Lets runs reference a version of their own organization only.
ALTER TABLE workflow_versions ADD CONSTRAINT workflow_versions_org_id UNIQUE (organization_id, id);

-- A run pins one published version, which is never deleted while runs reference it.
CREATE TABLE runs (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    workflow_id uuid NOT NULL,
    workflow_version_id uuid NOT NULL,
    version integer NOT NULL,
    trigger_source text NOT NULL DEFAULT 'manual' CHECK (trigger_source IN ('manual')),
    input jsonb NOT NULL,
    status text NOT NULL DEFAULT 'queued'
        CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'canceled')),
    cancel_requested_at timestamptz,
    rerun_of uuid REFERENCES runs(id) ON DELETE SET NULL,
    created_by uuid REFERENCES users(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    started_at timestamptz,
    finished_at timestamptz,
    last_event_seq bigint NOT NULL DEFAULT 0,
    FOREIGN KEY (organization_id, workflow_id) REFERENCES workflows (organization_id, id) ON DELETE CASCADE,
    FOREIGN KEY (organization_id, workflow_version_id) REFERENCES workflow_versions (organization_id, id),
    CONSTRAINT runs_org_id UNIQUE (organization_id, id)
);
CREATE INDEX runs_workflow_created_idx ON runs (workflow_id, created_at DESC, id DESC);

-- One row per node of a run. `children` and `pending_parents` come from the plan: a node
-- that succeeds decrements each child once, and the child is queued when it reaches 0.
CREATE TABLE node_runs (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    organization_id uuid NOT NULL,
    run_id uuid NOT NULL,
    node_id text NOT NULL,
    executor text NOT NULL,
    state text NOT NULL CHECK (
        state IN (
            'blocked', 'queued', 'running', 'retry_wait',
            'succeeded', 'failed', 'canceled', 'skipped'
        )
    ),
    attempt integer NOT NULL DEFAULT 0,
    max_attempts integer NOT NULL CHECK (max_attempts BETWEEN 1 AND 5),
    timeout_seconds integer NOT NULL CHECK (timeout_seconds BETWEEN 1 AND 14400),
    pending_parents integer NOT NULL CHECK (pending_parents >= 0),
    children text[] NOT NULL DEFAULT '{}',
    available_at timestamptz NOT NULL DEFAULT now(),
    lease_token bigint NOT NULL DEFAULT 0,
    leased_by uuid,
    lease_expires_at timestamptz,
    input jsonb,
    output jsonb,
    error jsonb,
    log_bytes bigint NOT NULL DEFAULT 0,
    log_truncated boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    started_at timestamptz,
    finished_at timestamptz,
    FOREIGN KEY (organization_id, run_id) REFERENCES runs (organization_id, id) ON DELETE CASCADE,
    CONSTRAINT node_runs_run_node UNIQUE (run_id, node_id)
);
CREATE INDEX node_runs_claimable ON node_runs (available_at, created_at)
    WHERE state IN ('queued', 'retry_wait');
CREATE INDEX node_runs_expired ON node_runs (lease_expires_at) WHERE state = 'running';

-- Each run numbers its own events from 1 without gaps, so a stream can resume after any seq.
CREATE TABLE run_events (
    organization_id uuid NOT NULL,
    run_id uuid NOT NULL,
    seq bigint NOT NULL,
    payload jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (run_id, seq),
    FOREIGN KEY (organization_id, run_id) REFERENCES runs (organization_id, id) ON DELETE CASCADE
);

-- System-wide: workers serve every organization. Host contracts stay empty until plugins
-- are loaded at run time.
CREATE TABLE worker_instances (
    id uuid PRIMARY KEY,
    executors text[] NOT NULL,
    host_contracts text[] NOT NULL DEFAULT '{}',
    capacity integer NOT NULL CHECK (capacity >= 1),
    started_at timestamptz NOT NULL DEFAULT now(),
    last_heartbeat_at timestamptz NOT NULL DEFAULT now()
);

-- Wakes the API's event streams. The payload is the run id, never tenant data, and the
-- notifications of one transaction for one run arrive as one at commit.
CREATE FUNCTION corqty.notify_run_event()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM pg_notify('corqty_run_events', NEW.run_id::text);
    RETURN NULL;
END
$$;

REVOKE ALL ON FUNCTION corqty.notify_run_event() FROM PUBLIC;

CREATE TRIGGER run_events_notify AFTER INSERT ON run_events
    FOR EACH ROW EXECUTE FUNCTION corqty.notify_run_event();

-- Appends one event and returns its seq, or NULL when the caller cannot update the run. The
-- UPDATE holds the runs row lock until commit (writers of a run's node_runs take it first),
-- so concurrent appends number events without gaps and commit them in seq order. Only
-- top-level nulls are dropped: a null inside a node's output is data.
CREATE FUNCTION corqty.append_run_event(p_run_id uuid, p_payload jsonb)
RETURNS bigint
LANGUAGE sql
SET search_path = public, pg_temp
AS $$
    WITH run AS (
        UPDATE runs
        SET last_event_seq = last_event_seq + 1
        WHERE id = p_run_id
        RETURNING organization_id, last_event_seq
    )
    INSERT INTO run_events(organization_id, run_id, seq, payload)
    SELECT organization_id, p_run_id, last_event_seq,
           (p_payload - ARRAY(SELECT key FROM jsonb_each(p_payload) WHERE value = 'null'::jsonb))
               || jsonb_build_object('seq', last_event_seq)
    FROM run
    RETURNING seq
$$;

-- Finishes the run once none of its nodes can change: a failure wins, then a cancel request
-- or a canceled node, else success (the rule of corqty_domain::run::aggregate). A run
-- without nodes finishes at once. The terminal run.state event is the run's last event.
CREATE FUNCTION corqty.settle_run(p_run_id uuid)
RETURNS void
LANGUAGE plpgsql
SET search_path = public, pg_temp
AS $$
DECLARE
    v_status text;
BEGIN
    UPDATE runs AS r
    SET status = CASE
            WHEN nodes.failed > 0 THEN 'failed'
            WHEN r.cancel_requested_at IS NOT NULL OR nodes.canceled > 0 THEN 'canceled'
            ELSE 'succeeded'
        END,
        finished_at = now()
    FROM (
        SELECT count(*) FILTER (WHERE state = 'failed') AS failed,
               count(*) FILTER (WHERE state = 'canceled') AS canceled,
               count(*) FILTER (
                   WHERE state NOT IN ('succeeded', 'failed', 'canceled', 'skipped')
               ) AS pending
        FROM node_runs
        WHERE run_id = p_run_id
    ) AS nodes
    WHERE r.id = p_run_id
      AND r.status IN ('queued', 'running')
      AND nodes.pending = 0
    RETURNING r.status INTO v_status;
    IF FOUND THEN
        PERFORM corqty.append_run_event(
            p_run_id,
            jsonb_build_object('type', 'run.state', 'state', v_status)
        );
    END IF;
END
$$;

-- SECURITY INVOKER: called by corqty_api they run under its row-level security, and called
-- from the worker's definer functions they run as the owner.
REVOKE ALL ON FUNCTION corqty.append_run_event(uuid, jsonb) FROM PUBLIC;
REVOKE ALL ON FUNCTION corqty.settle_run(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.append_run_event(uuid, jsonb) TO corqty_api;
GRANT EXECUTE ON FUNCTION corqty.settle_run(uuid) TO corqty_api;

-- Workers write runs through definer functions, so corqty_api only creates, reads and
-- cancels them. Events are append-only, and nothing is deleted.
GRANT SELECT, INSERT, UPDATE ON runs, node_runs TO corqty_api;
GRANT SELECT, INSERT ON run_events TO corqty_api;

ALTER TABLE runs ENABLE ROW LEVEL SECURITY;
ALTER TABLE runs FORCE ROW LEVEL SECURITY;
CREATE POLICY runs_select ON runs
    FOR SELECT TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.is_member(corqty.current_organization_id()))
    );
CREATE POLICY runs_insert ON runs
    FOR INSERT TO corqty_api
    WITH CHECK (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_run(corqty.current_organization_id()))
    );
CREATE POLICY runs_update ON runs
    FOR UPDATE TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_run(corqty.current_organization_id()))
    )
    WITH CHECK (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_run(corqty.current_organization_id()))
    );

ALTER TABLE node_runs ENABLE ROW LEVEL SECURITY;
ALTER TABLE node_runs FORCE ROW LEVEL SECURITY;
CREATE POLICY node_runs_select ON node_runs
    FOR SELECT TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.is_member(corqty.current_organization_id()))
    );
CREATE POLICY node_runs_insert ON node_runs
    FOR INSERT TO corqty_api
    WITH CHECK (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_run(corqty.current_organization_id()))
    );
CREATE POLICY node_runs_update ON node_runs
    FOR UPDATE TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_run(corqty.current_organization_id()))
    )
    WITH CHECK (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_run(corqty.current_organization_id()))
    );

ALTER TABLE run_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE run_events FORCE ROW LEVEL SECURITY;
CREATE POLICY run_events_select ON run_events
    FOR SELECT TO corqty_api
    USING (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.is_member(corqty.current_organization_id()))
    );
CREATE POLICY run_events_insert ON run_events
    FOR INSERT TO corqty_api
    WITH CHECK (
        organization_id = corqty.current_organization_id()
        AND (SELECT corqty.can_run(corqty.current_organization_id()))
    );

-- No role reads worker_instances directly: the policy only satisfies the forced row-level
-- security guard, and corqty_api has no grant on the table.
ALTER TABLE worker_instances ENABLE ROW LEVEL SECURITY;
ALTER TABLE worker_instances FORCE ROW LEVEL SECURITY;
CREATE POLICY worker_instances_none ON worker_instances
    FOR SELECT TO corqty_api
    USING (false);
