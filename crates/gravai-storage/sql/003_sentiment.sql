-- Add sentiment columns to utterances table
ALTER TABLE utterances ADD COLUMN sentiment_label TEXT;
ALTER TABLE utterances ADD COLUMN sentiment_score REAL;
ALTER TABLE utterances ADD COLUMN emotions_json TEXT;
