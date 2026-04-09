-- Correction columns for utterances: raw ASR text is preserved, corrections live here.
ALTER TABLE utterances ADD COLUMN corrected_text TEXT;
ALTER TABLE utterances ADD COLUMN correction_status TEXT;    -- NULL | 'pending' | 'done' | 'error'
ALTER TABLE utterances ADD COLUMN correction_provider TEXT;  -- e.g. "ollama/llama3"
ALTER TABLE utterances ADD COLUMN corrected_at TEXT;         -- ISO timestamp
