ALTER TABLE node_runs ADD COLUMN log_limit_bytes integer NOT NULL DEFAULT 10485760 CHECK (log_limit_bytes BETWEEN 1 AND 10485760);

CREATE OR REPLACE FUNCTION corqty.append_node_log(
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
    v_limit integer;
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
    SELECT node_id, log_bytes, log_truncated, log_limit_bytes INTO v_node, v_logged, v_truncated, v_limit
    FROM node_runs
    WHERE id = p_node_run_id;
    IF v_truncated THEN
        RETURN true;
    END IF;

    v_kept := least(length(v_bytes), v_limit - v_logged);
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

