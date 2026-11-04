-- A worker renews the lease of each node run it executes (corqty.heartbeat). One that crashed
-- or lost its connection stops renewing, and its node runs stay running past
-- lease_expires_at. Every API replica calls corqty.recover_expired_leases() every few
-- seconds. The replica that takes the advisory lock recovers them; the others skip the pass.

-- Ends each expired lease as an attempt that failed with a retryable lease_expired error (see
-- corqty.finish_node_run). The node waits for a retry while attempts are left, fails and
-- skips its descendants once none are, and is canceled when its run was asked to cancel.
-- The claim already counted the attempt, so it is not counted again. A new lease token fences
-- out the old worker, should it come back. Returns how many node runs it recovered, and 0
-- when another caller holds the lock.
CREATE FUNCTION corqty.recover_expired_leases()
RETURNS integer
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = public, pg_temp
AS $$
DECLARE
    v_run uuid;
    v_node_run uuid;
    v_recovered integer := 0;
BEGIN
    -- One caller at a time. The lock ends with the transaction.
    IF NOT pg_try_advisory_xact_lock(7243, 1) THEN
        RETURN 0;
    END IF;
    FOR v_run IN
        SELECT DISTINCT run_id
        FROM node_runs
        WHERE state = 'running' AND lease_expires_at < now()
        ORDER BY run_id
    LOOP
        -- The run first, then its expired node runs in one statement in id order. Under the
        -- locks, a lease that was completed or renewed in the meantime no longer matches.
        PERFORM FROM runs WHERE id = v_run FOR UPDATE;
        FOR v_node_run IN
            WITH fenced AS (
                UPDATE node_runs AS n
                SET lease_token = n.lease_token + 1
                FROM (
                    SELECT id
                    FROM node_runs
                    WHERE run_id = v_run AND state = 'running' AND lease_expires_at < now()
                    ORDER BY id
                    FOR UPDATE
                ) AS expired
                WHERE n.id = expired.id
                RETURNING n.id
            )
            SELECT id FROM fenced ORDER BY id
        LOOP
            PERFORM corqty.finish_node_run(v_node_run, '{
                "status": "failed",
                "error": {
                    "code": "lease_expired",
                    "message": "The worker stopped renewing its lease.",
                    "retryable": true
                }
            }');
            v_recovered := v_recovered + 1;
        END LOOP;
    END LOOP;
    RETURN v_recovered;
END
$$;

REVOKE ALL ON FUNCTION corqty.recover_expired_leases() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION corqty.recover_expired_leases() TO corqty_api;
