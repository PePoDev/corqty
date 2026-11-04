-- Workers reach run data only through the definer functions below, granted to corqty_worker
-- one by one. Every call after the claim is fenced: the node run must be running under the
-- caller's worker id and the lease token of its claim. A worker that lost its lease, or that
-- names another worker's or another organization's node run, changes nothing.

CREATE TYPE corqty.claimed_job AS (
    node_run_id uuid,
    run_id uuid,
    organization_id uuid,
    executor text,
    attempt integer,
    lease_token bigint
);

-- Records or refreshes a worker process and the executors it runs, as "<key>@<version>".
CREATE FUNCTION corqty.register_worker(
    p_worker_id uuid,
    p_executors text[],
    p_host_contracts text[],
    p_capacity integer
)
RETURNS void
LANGUAGE sql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    INSERT INTO worker_instances(id, executors, host_contracts, capacity)
    VALUES (p_worker_id, p_executors, p_host_contracts, p_capacity)
    ON CONFLICT (id) DO UPDATE
    SET executors = EXCLUDED.executors,
        host_contracts = EXCLUDED.host_contracts,
        capacity = EXCLUDED.capacity,
        last_heartbeat_at = now()
$$;

-- Leases up to p_limit due node runs of the worker's executors, oldest first. A due retry is
-- claimed straight from retry_wait, and each claim consumes an attempt. Nothing of a run that
-- was asked to cancel is handed out. Every call, even with p_limit 0, counts as a heartbeat.
-- SKIP LOCKED lets workers claim side by side; no runs row is locked.
CREATE FUNCTION corqty.claim_node_runs(p_worker_id uuid, p_limit integer, p_lease_seconds integer)
RETURNS SETOF corqty.claimed_job
LANGUAGE sql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    WITH worker AS (
        UPDATE worker_instances
        SET last_heartbeat_at = now()
        WHERE id = p_worker_id
        RETURNING executors
    ),
    due AS (
        SELECT n.id
        FROM node_runs AS n
        JOIN runs AS r ON r.id = n.run_id
        JOIN worker AS w ON n.executor = ANY (w.executors)
        WHERE n.state IN ('queued', 'retry_wait')
          AND n.available_at <= now()
          AND r.cancel_requested_at IS NULL
        ORDER BY n.available_at, n.created_at
        LIMIT p_limit
        FOR UPDATE OF n SKIP LOCKED
    )
    UPDATE node_runs AS n
    SET state = 'running',
        attempt = n.attempt + 1,
        lease_token = n.lease_token + 1,
        leased_by = p_worker_id,
        lease_expires_at = now() + make_interval(secs => p_lease_seconds)
    FROM due
    WHERE n.id = due.id
    RETURNING n.id, n.run_id, n.organization_id, n.executor, n.attempt, n.lease_token
$$;

-- Locks the run, then the node run, if the worker holds the node run's lease, and returns
-- the run id; NULL when the fence does not match. Every writer of a run takes its runs row
-- first, so writers of one run never deadlock.
CREATE FUNCTION corqty.lock_lease(p_worker_id uuid, p_node_run_id uuid, p_lease_token bigint)
RETURNS uuid
LANGUAGE plpgsql
SET search_path = public, pg_temp
AS $$
DECLARE
    v_run uuid;
BEGIN
    -- node_runs.run_id never changes, so it is read without a lock, and a caller without the
    -- lease locks nothing.
    SELECT run_id INTO v_run
    FROM node_runs
    WHERE id = p_node_run_id
      AND leased_by = p_worker_id
      AND lease_token = p_lease_token
      AND state = 'running';
    IF NOT FOUND THEN
        RETURN NULL;
    END IF;
    PERFORM FROM runs WHERE id = v_run FOR UPDATE;
    -- The fence again under the lock: the lease may have ended in the meantime.
    PERFORM FROM node_runs
    WHERE id = p_node_run_id
      AND leased_by = p_worker_id
      AND lease_token = p_lease_token
      AND state = 'running'
    FOR UPDATE;
    IF NOT FOUND THEN
        RETURN NULL;
    END IF;
    RETURN v_run;
END
$$;

-- Moves a running node run on after an attempt, inside the caller's lock on its run.
-- p_outcome is {"status": "succeeded", "input": {..}, "output": {..}},
-- {"status": "failed", "input"?: {..}, "error": {code, message, retryable}} or
-- {"status": "canceled"}. A retryable failure with attempts left waits in retry_wait with
-- exponential backoff from 2 s (20% jitter, at most 300 s), unless the run was asked to
-- cancel: then the failure, like a canceled attempt, cancels the node and keeps the error.
-- A success releases each child whose parents have all succeeded; a failed or canceled node
-- skips its blocked descendants. Each change is one node.state event, then the run settles.
CREATE FUNCTION corqty.finish_node_run(p_node_run_id uuid, p_outcome jsonb)
RETURNS void
LANGUAGE plpgsql
SET search_path = public, pg_temp
AS $$
DECLARE
    v_run uuid;
    v_state text;
    v_children text[];
    v_event jsonb;
BEGIN
    SELECT n.run_id,
           CASE
               WHEN p_outcome ->> 'status' = 'succeeded' THEN 'succeeded'
               WHEN p_outcome ->> 'status' = 'canceled' OR r.cancel_requested_at IS NOT NULL
                   THEN 'canceled'
               WHEN (p_outcome #>> '{error,retryable}')::boolean AND n.attempt < n.max_attempts
                   THEN 'retry_wait'
               ELSE 'failed'
           END
    INTO v_run, v_state
    FROM node_runs AS n
    JOIN runs AS r ON r.id = n.run_id
    WHERE n.id = p_node_run_id;

    UPDATE node_runs
    SET state = v_state,
        input = coalesce(p_outcome -> 'input', input),
        output = CASE WHEN v_state = 'succeeded' THEN p_outcome -> 'output' END,
        error = p_outcome -> 'error',
        available_at = CASE
            WHEN v_state = 'retry_wait' THEN now() + make_interval(
                secs => least(300, 2 * power(2, attempt - 1) * (1 + 0.2 * random()))
            )
            ELSE available_at
        END,
        leased_by = NULL,
        lease_expires_at = NULL,
        finished_at = CASE WHEN v_state = 'retry_wait' THEN NULL ELSE now() END
    WHERE id = p_node_run_id
    RETURNING children,
              jsonb_build_object(
                  'type', 'node.state', 'nodeId', node_id, 'state', state,
                  'attempt', attempt, 'error', error, 'output', output
              )
    INTO v_children, v_event;
    PERFORM corqty.append_run_event(v_run, v_event);

    IF v_state = 'succeeded' THEN
        -- One statement locks the children in id order. Each parent decrements a child once,
        -- and the child is queued when no parent is left.
        FOR v_event IN
            WITH released AS (
                UPDATE node_runs AS child
                SET pending_parents = child.pending_parents - 1,
                    state = CASE WHEN child.pending_parents = 1 THEN 'queued' ELSE 'blocked' END,
                    available_at = now()
                FROM (
                    SELECT id
                    FROM node_runs
                    WHERE run_id = v_run AND node_id = ANY (v_children) AND state = 'blocked'
                    ORDER BY id
                    FOR UPDATE
                ) AS waiting
                WHERE child.id = waiting.id
                RETURNING child.id, child.node_id, child.state, child.attempt
            )
            SELECT jsonb_build_object(
                       'type', 'node.state', 'nodeId', node_id, 'state', state,
                       'attempt', attempt
                   )
            FROM released
            WHERE state = 'queued'
            ORDER BY id
        LOOP
            PERFORM corqty.append_run_event(v_run, v_event);
        END LOOP;
    ELSIF v_state IN ('failed', 'canceled') THEN
        FOR v_event IN
            WITH RECURSIVE below(node_id) AS (
                SELECT unnest(v_children)
                UNION
                SELECT child
                FROM below
                JOIN node_runs AS n ON n.run_id = v_run AND n.node_id = below.node_id
                CROSS JOIN unnest(n.children) AS child
            ),
            skipped AS (
                UPDATE node_runs AS descendant
                SET state = 'skipped', finished_at = now()
                FROM (
                    SELECT id
                    FROM node_runs
                    WHERE run_id = v_run
                      AND node_id IN (SELECT node_id FROM below)
                      AND state = 'blocked'
                    ORDER BY id
                    FOR UPDATE
                ) AS waiting
                WHERE descendant.id = waiting.id
                RETURNING descendant.id, descendant.node_id, descendant.attempt
            )
            SELECT jsonb_build_object(
                       'type', 'node.state', 'nodeId', node_id, 'state', 'skipped',
                       'attempt', attempt
                   )
            FROM skipped
            ORDER BY id
        LOOP
            PERFORM corqty.append_run_event(v_run, v_event);
        END LOOP;
    END IF;

    PERFORM corqty.settle_run(v_run);
END
$$;

-- Starts a claimed attempt and returns its job, or NULL when the fence does not match. The
-- first start of a run moves it to running. The definition is the system one or the run's
-- own organization's: organization keys never start with "corqty.", so one row matches.
CREATE FUNCTION corqty.start_node_run(p_worker_id uuid, p_node_run_id uuid, p_lease_token bigint)
RETURNS jsonb
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
DECLARE
    v_run uuid := corqty.lock_lease(p_worker_id, p_node_run_id, p_lease_token);
    v_event jsonb;
    v_job jsonb;
BEGIN
    IF v_run IS NULL THEN
        RETURN NULL;
    END IF;
    UPDATE runs SET status = 'running', started_at = now() WHERE id = v_run AND status = 'queued';
    IF FOUND THEN
        PERFORM corqty.append_run_event(v_run, '{"type": "run.state", "state": "running"}');
    END IF;
    UPDATE node_runs
    SET started_at = now()
    WHERE id = p_node_run_id
    RETURNING jsonb_build_object(
        'type', 'node.state', 'nodeId', node_id, 'state', 'running', 'attempt', attempt
    )
    INTO v_event;
    PERFORM corqty.append_run_event(v_run, v_event);

    SELECT jsonb_build_object(
               'executionId', n.id,
               'attempt', n.attempt,
               'maxAttempts', n.max_attempts,
               'timeoutSeconds', n.timeout_seconds,
               'workflowId', r.workflow_id,
               'runId', r.id,
               'nodeId', n.node_id,
               'cancelRequested', r.cancel_requested_at IS NOT NULL,
               'runInput', r.input,
               'graph', v.graph,
               'definition', (
                   SELECT d.spec
                   FROM jsonb_array_elements(v.graph -> 'nodes') AS node
                   JOIN node_definitions AS d
                     ON d.key = node #>> '{definition,key}'
                    AND d.version = (node #>> '{definition,version}')::integer
                   WHERE node ->> 'id' = n.node_id
                     AND (d.organization_id IS NULL OR d.organization_id = n.organization_id)
               ),
               'upstreamOutputs', (
                   SELECT coalesce(jsonb_object_agg(parent.node_id, parent.output), '{}')
                   FROM node_runs AS parent
                   WHERE parent.run_id = n.run_id
                     AND n.node_id = ANY (parent.children)
                     AND jsonb_typeof(parent.output) = 'object'
               )
           )
    INTO v_job
    FROM node_runs AS n
    JOIN runs AS r ON r.id = n.run_id
    JOIN workflow_versions AS v ON v.id = r.workflow_version_id
    WHERE n.id = p_node_run_id;
    RETURN v_job;
END
$$;

-- Extends each lease the worker still holds and says whether its run was asked to cancel.
-- One row per lease asked about, in order; alive is false for a lease that is gone. It locks
-- only the worker's own running rows, in id order, and no runs row.
CREATE FUNCTION corqty.heartbeat(
    p_worker_id uuid,
    p_node_run_ids uuid[],
    p_lease_tokens bigint[],
    p_lease_seconds integer
)
RETURNS TABLE (node_run_id uuid, alive boolean, cancel_requested boolean)
LANGUAGE sql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    WITH worker AS (
        UPDATE worker_instances SET last_heartbeat_at = now() WHERE id = p_worker_id
    ),
    renewed AS (
        UPDATE node_runs AS n
        SET lease_expires_at = now() + make_interval(secs => p_lease_seconds)
        FROM (
            SELECT own.id
            FROM node_runs AS own
            JOIN unnest(p_node_run_ids, p_lease_tokens) AS lease(id, token)
              ON lease.id = own.id AND lease.token = own.lease_token
            WHERE own.leased_by = p_worker_id AND own.state = 'running'
            ORDER BY own.id
            FOR UPDATE OF own
        ) AS held
        WHERE n.id = held.id
        RETURNING n.id, n.run_id
    )
    SELECT lease.id, renewed.id IS NOT NULL, r.cancel_requested_at IS NOT NULL
    FROM unnest(p_node_run_ids) WITH ORDINALITY AS lease(id, position)
    LEFT JOIN renewed ON renewed.id = lease.id
    LEFT JOIN runs AS r ON r.id = renewed.run_id
    ORDER BY lease.position
$$;

-- Stores a batch of log text as one node.log event. A node run keeps at most 10 MiB of log
-- text (MAX_LOG_BYTES_PER_NODE_RUN): the batch that crosses the cap keeps its longest prefix
-- that fits without splitting a character, then one node.log_truncated event follows and
-- later text is dropped. True while the lease holds, stored or not; false once it is lost.
CREATE FUNCTION corqty.append_node_log(
    p_worker_id uuid,
    p_node_run_id uuid,
    p_lease_token bigint,
    p_stream text,
    p_text text
)
RETURNS boolean
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
DECLARE
    v_run uuid;
    v_node text;
    v_logged bigint;
    v_truncated boolean;
    v_bytes bytea := convert_to(p_text, 'UTF8');
    v_kept integer;
BEGIN
    IF p_stream NOT IN ('stdout', 'stderr', 'system') THEN
        RAISE EXCEPTION 'unknown log stream %', p_stream USING ERRCODE = 'invalid_parameter_value';
    END IF;
    v_run := corqty.lock_lease(p_worker_id, p_node_run_id, p_lease_token);
    IF v_run IS NULL THEN
        RETURN false;
    END IF;
    SELECT node_id, log_bytes, log_truncated INTO v_node, v_logged, v_truncated
    FROM node_runs
    WHERE id = p_node_run_id;
    IF v_truncated THEN
        RETURN true;
    END IF;

    v_kept := least(length(v_bytes), 10485760 - v_logged);
    -- Back off while the first byte left out continues a UTF-8 character (0b10xxxxxx).
    WHILE v_kept < length(v_bytes) AND get_byte(v_bytes, v_kept) & 192 = 128 LOOP
        v_kept := v_kept - 1;
    END LOOP;
    UPDATE node_runs
    SET log_bytes = log_bytes + v_kept,
        log_truncated = v_kept < length(v_bytes)
    WHERE id = p_node_run_id;
    IF v_kept > 0 THEN
        PERFORM corqty.append_run_event(v_run, jsonb_build_object(
            'type', 'node.log', 'nodeId', v_node, 'stream', p_stream,
            'text', convert_from(substring(v_bytes FROM 1 FOR v_kept), 'UTF8')
        ));
    END IF;
    IF v_kept < length(v_bytes) THEN
        PERFORM corqty.append_run_event(v_run, jsonb_build_object(
            'type', 'node.log_truncated', 'nodeId', v_node
        ));
    END IF;
    RETURN true;
END
$$;

-- Records how an attempt ended (see finish_node_run). False when the fence does not match.
CREATE FUNCTION corqty.complete_node_run(
    p_worker_id uuid,
    p_node_run_id uuid,
    p_lease_token bigint,
    p_outcome jsonb
)
RETURNS boolean
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
BEGIN
    IF corqty.lock_lease(p_worker_id, p_node_run_id, p_lease_token) IS NULL THEN
        RETURN false;
    END IF;
    PERFORM corqty.finish_node_run(p_node_run_id, p_outcome);
    RETURN true;
END
$$;

-- The executors that a worker heard from in the last minute runs, so the API can tell a
-- queued node that no live worker can take. The API cannot read worker_instances itself.
CREATE FUNCTION corqty.live_executors()
RETURNS text[]
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
    SELECT coalesce(array_agg(DISTINCT executor ORDER BY executor), '{}')
    FROM worker_instances, unnest(executors) AS executor
    WHERE last_heartbeat_at > now() - interval '60 seconds'
$$;

-- lock_lease and finish_node_run are SECURITY INVOKER helpers of the definer functions,
-- which call them as the owner. No role may call them directly.
REVOKE ALL ON FUNCTION
    corqty.register_worker(uuid, text[], text[], integer),
    corqty.claim_node_runs(uuid, integer, integer),
    corqty.lock_lease(uuid, uuid, bigint),
    corqty.finish_node_run(uuid, jsonb),
    corqty.start_node_run(uuid, uuid, bigint),
    corqty.heartbeat(uuid, uuid[], bigint[], integer),
    corqty.append_node_log(uuid, uuid, bigint, text, text),
    corqty.complete_node_run(uuid, uuid, bigint, jsonb),
    corqty.live_executors()
FROM PUBLIC;
GRANT EXECUTE ON FUNCTION
    corqty.register_worker(uuid, text[], text[], integer),
    corqty.claim_node_runs(uuid, integer, integer),
    corqty.start_node_run(uuid, uuid, bigint),
    corqty.heartbeat(uuid, uuid[], bigint[], integer),
    corqty.append_node_log(uuid, uuid, bigint, text, text),
    corqty.complete_node_run(uuid, uuid, bigint, jsonb)
TO corqty_worker;
GRANT EXECUTE ON FUNCTION corqty.live_executors() TO corqty_api;
