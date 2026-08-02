CREATE TABLE audit_logs (
    id BIGINT PRIMARY KEY,
    count INT NOT NULL DEFAULT 0
);

CREATE FUNCTION update_audit_count(
    p_id BIGINT,
    p_count INT
)
RETURNS INT
LANGUAGE plpgsql
AS $$
DECLARE
    v_old_count INT;
    v_result INT;
BEGIN
    -- Get the current count
    SELECT count INTO v_old_count
    FROM audit_logs
    WHERE id = p_id;
    
    -- Case 1: Insert new log and return 0 for count
    IF v_old_count IS NULL THEN
        INSERT INTO audit_logs (id, count)
        VALUES (p_id, p_count);
        RETURN 0;
    END IF;
    
    -- Case 2: No change, return nothing
    IF v_old_count = p_count THEN
        RETURN NULL;
    END IF;
    
    -- Case 3: Update and return the old value to send the message
    UPDATE audit_logs
    SET count = p_count
    WHERE id = p_id
    RETURNING count INTO v_result;
    
    RETURN v_old_count;
END;
$$;