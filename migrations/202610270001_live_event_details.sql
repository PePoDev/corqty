-- The node.state event that finish_node_run writes when an attempt ends also carries the
-- node's input, so a live run view shows it without reading the run again. An input that was
-- never resolved is NULL and, like every top-level null, is left out of the event. The rest of
-- the function is the one that 202610260002 created. CREATE OR REPLACE keeps its owner and its
-- privileges: no role may call it directly.
CREATE OR REPLACE FUNCTION corqty.finish_node_run(p_node_run_id uuid, p_outcome jsonb)
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
                  'attempt', attempt, 'error', error, 'output', output, 'input', input
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
