-- Queue-level scheduling: items with run_after > NOW() are not eligible for claim.
-- Used by dm_cleanup to delay action prompts so DM messages settle first.
ALTER TABLE message_queue ADD COLUMN run_after TIMESTAMPTZ;
