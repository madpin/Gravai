-- Settings a recording started with: capture, speech, assistant, sessions.
-- JSON from gravai_config::setups::Snapshot. NULL for older sessions.
ALTER TABLE sessions ADD COLUMN config_snapshot TEXT;
