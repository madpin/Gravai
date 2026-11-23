//! Session-scoped speaker identities.
//!
//! Live labeling matches each turn to a centroid. Stopping a recording
//! clusters every stored embedding once, so an early mistake can be corrected
//! without touching a name the user set.

use std::collections::{HashMap, HashSet};

/// Cosine similarity that keeps two turns on the same person.
pub(crate) const MATCH_THRESHOLD: f32 = 0.75;
const HYSTERESIS: f32 = 0.05;
const MIN_ENROLL_MS: u64 = 1_000;
const EMA_ALPHA: f32 = 0.1;

struct Centroid {
    label: String,
    vector: Vec<f32>,
}

/// Remotes remembered for one recording.
pub struct SpeakerBank {
    max_speakers: usize,
    centroids: Vec<Centroid>,
    last_label: Option<String>,
}

impl SpeakerBank {
    pub fn new(max_speakers: usize) -> Self {
        Self {
            max_speakers: max_speakers.max(1),
            centroids: Vec::new(),
            last_label: None,
        }
    }

    pub fn speaker_count(&self) -> usize {
        self.centroids.len()
    }

    /// Label one turn. A clip shorter than a second never opens a new Remote.
    pub fn assign(&mut self, embedding: &[f32], duration_ms: u64) -> String {
        let Some(embedding) = unit(embedding) else {
            return "Remote".to_string();
        };

        if self.centroids.is_empty() {
            if duration_ms < MIN_ENROLL_MS {
                return "Remote".to_string();
            }
            return self.enroll(embedding);
        }

        let scores: Vec<f32> = self
            .centroids
            .iter()
            .map(|centroid| dot(&centroid.vector, &embedding))
            .collect();
        let best = argmax(&scores);
        let best_score = scores[best];

        if best_score < MATCH_THRESHOLD
            && duration_ms >= MIN_ENROLL_MS
            && self.centroids.len() < self.max_speakers
        {
            return self.enroll(embedding);
        }

        let mut chosen = best;
        if best_score >= MATCH_THRESHOLD {
            if let Some(last) = &self.last_label {
                if let Some(last_idx) = self
                    .centroids
                    .iter()
                    .position(|centroid| &centroid.label == last)
                {
                    if scores[last_idx] + HYSTERESIS >= best_score {
                        chosen = last_idx;
                    }
                }
            }
            if duration_ms >= MIN_ENROLL_MS {
                update_centroid(&mut self.centroids[chosen].vector, &embedding);
            }
        }

        let label = self.centroids[chosen].label.clone();
        self.last_label = Some(label.clone());
        label
    }

    fn enroll(&mut self, embedding: Vec<f32>) -> String {
        let label = format!("Remote {}", self.centroids.len() + 1);
        self.centroids.push(Centroid {
            label: label.clone(),
            vector: embedding,
        });
        self.last_label = Some(label.clone());
        label
    }
}

/// One stored turn, ready to cluster when the recording stops.
pub struct EmbeddingSample {
    pub utterance_id: i64,
    pub embedding: Vec<f32>,
    /// Set when the user renamed this utterance's speaker. That name is kept.
    pub locked_name: Option<String>,
    pub start_ms: i64,
}

/// A new automatic label for one utterance.
pub struct SpeakerAssignment {
    pub utterance_id: i64,
    pub speaker: String,
}

/// Merge turns that sound alike, capped at `max_speakers`.
/// Locked names stay put and absorb matching automatic turns.
pub fn cluster_speaker_embeddings(
    samples: &[EmbeddingSample],
    max_speakers: usize,
) -> Vec<SpeakerAssignment> {
    let max_speakers = max_speakers.max(1);
    let usable: Vec<(usize, Vec<f32>)> = samples
        .iter()
        .enumerate()
        .filter_map(|(index, sample)| unit(&sample.embedding).map(|vector| (index, vector)))
        .collect();
    if usable.is_empty() {
        return Vec::new();
    }

    struct Cluster {
        members: Vec<usize>,
        centroid: Vec<f32>,
        locked_name: Option<String>,
        start_ms: i64,
    }

    let mut locked_groups: HashMap<String, Vec<usize>> = HashMap::new();
    let mut unlocked: Vec<usize> = Vec::new();
    for (index, _) in &usable {
        let sample = &samples[*index];
        if let Some(name) = sample.locked_name.as_ref().filter(|name| !name.is_empty()) {
            locked_groups.entry(name.clone()).or_default().push(*index);
        } else {
            unlocked.push(*index);
        }
    }

    let vector_of = |index: usize| -> Vec<f32> {
        usable
            .iter()
            .find(|(idx, _)| *idx == index)
            .map(|(_, vector)| vector.clone())
            .unwrap_or_default()
    };

    let mut clusters: Vec<Cluster> = Vec::new();
    for (name, members) in locked_groups {
        let start_ms = members
            .iter()
            .map(|index| samples[*index].start_ms)
            .min()
            .unwrap_or(0);
        let centroid = mean_vector(&members, &vector_of);
        clusters.push(Cluster {
            members,
            centroid,
            locked_name: Some(name),
            start_ms,
        });
    }
    for index in unlocked {
        clusters.push(Cluster {
            members: vec![index],
            centroid: vector_of(index),
            locked_name: None,
            start_ms: samples[index].start_ms,
        });
    }

    while clusters.len() > 1 {
        let mut best: Option<(usize, usize, f32)> = None;
        for i in 0..clusters.len() {
            for j in (i + 1)..clusters.len() {
                if clusters[i].locked_name.is_some() && clusters[j].locked_name.is_some() {
                    continue;
                }
                let similarity = dot(&clusters[i].centroid, &clusters[j].centroid);
                if best.is_none_or(|(_, _, best_sim)| similarity > best_sim) {
                    best = Some((i, j, similarity));
                }
            }
        }
        let Some((i, j, similarity)) = best else {
            break;
        };
        let over_cap = clusters.len() > max_speakers;
        if similarity < MATCH_THRESHOLD && !over_cap {
            break;
        }
        let absorbed = clusters.remove(j);
        let keep = if j < i { i - 1 } else { i };
        let into = &mut clusters[keep];
        into.members.extend(absorbed.members);
        into.start_ms = into.start_ms.min(absorbed.start_ms);
        if into.locked_name.is_none() {
            into.locked_name = absorbed.locked_name;
        }
        into.centroid = mean_vector(&into.members, &vector_of);
    }

    let mut used: HashSet<String> = clusters
        .iter()
        .filter_map(|cluster| cluster.locked_name.clone())
        .collect();
    let mut order: Vec<usize> = (0..clusters.len()).collect();
    order.sort_by_key(|&index| {
        (
            clusters[index].locked_name.is_none(),
            clusters[index].start_ms,
        )
    });

    let mut assignments = Vec::new();
    let mut next = 1u32;
    for index in order {
        let cluster = &clusters[index];
        let label = if let Some(name) = &cluster.locked_name {
            name.clone()
        } else {
            loop {
                let candidate = format!("Remote {next}");
                next += 1;
                if used.insert(candidate.clone()) {
                    break candidate;
                }
            }
        };
        for member in &cluster.members {
            if samples[*member].locked_name.is_none() {
                assignments.push(SpeakerAssignment {
                    utterance_id: samples[*member].utterance_id,
                    speaker: label.clone(),
                });
            }
        }
    }
    assignments
}

fn mean_vector(members: &[usize], vector_of: &impl Fn(usize) -> Vec<f32>) -> Vec<f32> {
    let Some(first) = members.first() else {
        return Vec::new();
    };
    let dim = vector_of(*first).len();
    let mut acc = vec![0.0f32; dim];
    for index in members {
        for (slot, value) in acc.iter_mut().zip(vector_of(*index)) {
            *slot += value;
        }
    }
    unit(&acc).unwrap_or(acc)
}

fn update_centroid(centroid: &mut Vec<f32>, sample: &[f32]) {
    for (current, value) in centroid.iter_mut().zip(sample) {
        *current = (1.0 - EMA_ALPHA) * *current + EMA_ALPHA * *value;
    }
    if let Some(normalized) = unit(centroid) {
        *centroid = normalized;
    }
}

fn dot(left: &[f32], right: &[f32]) -> f32 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}

fn unit(values: &[f32]) -> Option<Vec<f32>> {
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    if !norm.is_finite() || norm < 1e-6 {
        return None;
    }
    Some(values.iter().map(|value| value / norm).collect())
}

fn argmax(scores: &[f32]) -> usize {
    scores
        .iter()
        .enumerate()
        .fold((0, f32::NEG_INFINITY), |(best_i, best), (i, score)| {
            if *score > best {
                (i, *score)
            } else {
                (best_i, best)
            }
        })
        .0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(id: i64, embedding: Vec<f32>, start_ms: i64) -> EmbeddingSample {
        EmbeddingSample {
            utterance_id: id,
            embedding,
            locked_name: None,
            start_ms,
        }
    }

    #[test]
    fn the_same_voice_keeps_one_remote() {
        let mut bank = SpeakerBank::new(4);
        assert_eq!(bank.assign(&[1.0, 0.0], 2_000), "Remote 1");
        assert_eq!(bank.assign(&[0.99, 0.14106736], 2_000), "Remote 1");
        assert_eq!(bank.speaker_count(), 1);
    }

    #[test]
    fn a_second_voice_opens_the_next_remote() {
        let mut bank = SpeakerBank::new(4);
        assert_eq!(bank.assign(&[1.0, 0.0, 0.0], 2_000), "Remote 1");
        assert_eq!(bank.assign(&[0.0, 1.0, 0.0], 2_000), "Remote 2");
        assert_eq!(bank.speaker_count(), 2);
    }

    #[test]
    fn a_short_clip_does_not_open_a_remote() {
        let mut bank = SpeakerBank::new(4);
        assert_eq!(bank.assign(&[1.0, 0.0], 400), "Remote");
        assert_eq!(bank.speaker_count(), 0);
        assert_eq!(bank.assign(&[1.0, 0.0], 2_000), "Remote 1");
        assert_eq!(bank.assign(&[0.0, 1.0], 400), "Remote 1");
        assert_eq!(bank.speaker_count(), 1);
        assert_eq!(bank.assign(&[0.0, 1.0], 2_000), "Remote 2");
    }

    #[test]
    fn the_cap_assigns_to_the_nearest_voice() {
        let mut bank = SpeakerBank::new(2);
        assert_eq!(bank.assign(&[1.0, 0.0, 0.0], 2_000), "Remote 1");
        assert_eq!(bank.assign(&[0.0, 1.0, 0.0], 2_000), "Remote 2");
        assert_eq!(bank.assign(&[0.0, 0.0, 1.0], 2_000), "Remote 1");
        assert_eq!(bank.speaker_count(), 2);
    }

    #[test]
    fn a_near_tie_keeps_the_previous_remote() {
        let mut bank = SpeakerBank::new(4);
        let second = {
            let y = (1.0 - 0.70f32 * 0.70).sqrt();
            vec![0.70, y]
        };
        assert_eq!(bank.assign(&[1.0, 0.0], 2_000), "Remote 1");
        assert_eq!(bank.assign(&second, 2_000), "Remote 2");
        let y = (1.0 - 0.93f32 * 0.93).sqrt();
        assert_eq!(bank.assign(&[0.93, y], 2_000), "Remote 2");
    }

    #[test]
    fn clustering_merges_a_repeated_voice_and_keeps_a_different_one() {
        let samples = vec![
            remote(1, vec![1.0, 0.0], 0),
            remote(2, vec![0.98, 0.2], 1_000),
            remote(3, vec![0.0, 1.0], 2_000),
            remote(4, vec![0.1, 0.99], 3_000),
        ];
        let mut labels: Vec<_> = cluster_speaker_embeddings(&samples, 4)
            .into_iter()
            .map(|assignment| (assignment.utterance_id, assignment.speaker))
            .collect();
        labels.sort_by_key(|(id, _)| *id);
        assert_eq!(labels[0].1, labels[1].1);
        assert_eq!(labels[2].1, labels[3].1);
        assert_ne!(labels[0].1, labels[2].1);
        assert_eq!(labels[0].1, "Remote 1");
        assert_eq!(labels[2].1, "Remote 2");
    }

    #[test]
    fn clustering_keeps_a_name_the_user_set() {
        let samples = vec![
            EmbeddingSample {
                utterance_id: 1,
                embedding: vec![1.0, 0.0],
                locked_name: Some("Alex".into()),
                start_ms: 0,
            },
            remote(2, vec![0.99, 0.1], 1_000),
            remote(3, vec![0.0, 1.0], 2_000),
        ];
        let assigned = cluster_speaker_embeddings(&samples, 4);
        let alex = assigned.iter().find(|a| a.utterance_id == 2).unwrap();
        let other = assigned.iter().find(|a| a.utterance_id == 3).unwrap();
        assert_eq!(alex.speaker, "Alex");
        assert_eq!(other.speaker, "Remote 1");
        assert!(assigned.iter().all(|a| a.utterance_id != 1));
    }

    #[test]
    fn clustering_respects_the_speaker_cap() {
        let samples = vec![
            remote(1, vec![1.0, 0.0, 0.0], 0),
            remote(2, vec![0.0, 1.0, 0.0], 1_000),
            remote(3, vec![0.0, 0.0, 1.0], 2_000),
        ];
        let assigned = cluster_speaker_embeddings(&samples, 1);
        assert!(assigned.iter().all(|a| a.speaker == "Remote 1"));
    }
}
