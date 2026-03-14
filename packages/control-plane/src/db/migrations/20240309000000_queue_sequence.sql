-- Strict FIFO ordering within the same priority bucket.
-- created_at has microsecond precision but can collide under load.
-- A BIGSERIAL guarantees monotonic insertion order.
ALTER TABLE message_queue ADD COLUMN seq BIGSERIAL;

-- Rebuild the worker index to use seq instead of created_at for ordering
DROP INDEX IF EXISTS idx_mq_agent_status;
CREATE INDEX idx_mq_agent_status ON message_queue(agent_id, status, priority, seq);
