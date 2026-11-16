// Pure helpers for aligning transcript/bookmark UI to the session timeline
// (session-time milliseconds), shared by TranscriptView and anything else
// that needs to place an utterance on the same timebase as audio playback.

/** An utterance as returned by `get_transcript`/`get_transcript_since`. */
export interface TranscriptUtterance {
  start_ms?: number | null;
  timestamp: string;
  [key: string]: unknown;
}

/**
 * Session-time offset (ms) of an utterance, matching the audio player's
 * timebase (0 = session start).
 *
 * Prefers `start_ms` (set by the transcription pipeline directly in
 * session-time, including the continuation-leg offset for continued
 * recordings). Falls back to the wall-clock timestamp minus the session's
 * start time for older rows that predate `start_ms`.
 */
export function getUtteranceOffsetMs(u: TranscriptUtterance, baseTimeMs: number): number {
  if (u.start_ms != null) return u.start_ms;
  const ts = new Date(u.timestamp).getTime();
  return ts - baseTimeMs;
}
