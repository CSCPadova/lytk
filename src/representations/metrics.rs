//! Objective metrics (EFT3), modeled on `muspy.metrics`.
//!
//! These summarise a piece for ML evaluation. All operate on a [`NoteArray`]
//! (the timed note list, in time steps with `resolution` steps per quarter
//! note). Following muspy, "no notes" cases return `NaN` for ratio/entropy
//! metrics. Drum handling is out of scope (the note-array has no drum flag).

use super::note_array::NoteArray;
use super::piano_roll::{to_piano_roll, PITCH_COUNT};

/// Number of unique MIDI pitches used.
pub fn n_pitches_used(arr: &NoteArray) -> usize {
    let mut seen = [false; PITCH_COUNT];
    let mut count = 0;
    for n in &arr.notes {
        let p = n.pitch as usize;
        if p < PITCH_COUNT && !seen[p] {
            seen[p] = true;
            count += 1;
        }
    }
    count
}

/// Number of unique pitch classes (0–11) used.
pub fn n_pitch_classes_used(arr: &NoteArray) -> usize {
    let mut seen = [false; 12];
    let mut count = 0;
    for n in &arr.notes {
        let pc = (n.pitch % 12) as usize;
        if !seen[pc] {
            seen[pc] = true;
            count += 1;
        }
    }
    count
}

/// Pitch range (highest − lowest MIDI pitch); 0 when there are no notes.
pub fn pitch_range(arr: &NoteArray) -> u8 {
    let mut lowest = 127u8;
    let mut highest = 0u8;
    let mut any = false;
    for n in &arr.notes {
        any = true;
        highest = highest.max(n.pitch);
        lowest = lowest.min(n.pitch);
    }
    if !any {
        return 0;
    }
    highest - lowest
}

/// Normalised pitch-class histogram (12 bins summing to 1); all zeros when
/// there are no notes.
pub fn pitch_class_histogram(arr: &NoteArray) -> [f64; 12] {
    let mut counter = [0.0f64; 12];
    for n in &arr.notes {
        counter[(n.pitch % 12) as usize] += 1.0;
    }
    let total: f64 = counter.iter().sum();
    if total > 0.0 {
        for c in &mut counter {
            *c /= total;
        }
    }
    counter
}

/// Shannon entropy (base 2) of a probability vector, ignoring zero entries.
fn entropy(probs: &[f64]) -> f64 {
    let mut h = 0.0;
    for &p in probs {
        if p > 0.0 {
            h -= p * p.log2();
        }
    }
    h
}

/// Shannon entropy of the normalised pitch histogram (`NaN` if no notes).
pub fn pitch_entropy(arr: &NoteArray) -> f64 {
    let mut counter = [0.0f64; PITCH_COUNT];
    for n in &arr.notes {
        counter[n.pitch as usize] += 1.0;
    }
    let total: f64 = counter.iter().sum();
    if total < 1.0 {
        return f64::NAN;
    }
    let probs: Vec<f64> = counter.iter().map(|&c| c / total).collect();
    entropy(&probs)
}

/// Shannon entropy of the normalised pitch-class histogram (`NaN` if no notes).
pub fn pitch_class_entropy(arr: &NoteArray) -> f64 {
    let mut counter = [0.0f64; 12];
    for n in &arr.notes {
        counter[(n.pitch % 12) as usize] += 1.0;
    }
    let total: f64 = counter.iter().sum();
    if total < 1.0 {
        return f64::NAN;
    }
    let probs: Vec<f64> = counter.iter().map(|&c| c / total).collect();
    entropy(&probs)
}

/// Number of pitches sounding at each time step, from the binary piano-roll.
fn pitches_per_step(arr: &NoteArray) -> Vec<u32> {
    let pr = to_piano_roll(arr, false);
    (0..pr.num_steps as usize)
        .map(|step| {
            pr.data[step * PITCH_COUNT..(step + 1) * PITCH_COUNT]
                .iter()
                .filter(|&&c| c != 0)
                .count() as u32
        })
        .collect()
}

/// Average number of pitches sounding at steps where at least one is on
/// (`NaN` if no notes).
pub fn polyphony(arr: &NoteArray) -> f64 {
    let per_step = pitches_per_step(arr);
    let denominator = per_step.iter().filter(|&&c| c > 0).count();
    if denominator < 1 {
        return f64::NAN;
    }
    let total: u32 = per_step.iter().sum();
    total as f64 / denominator as f64
}

/// Ratio of time steps where more than `threshold` pitches sound (`NaN` if the
/// piece has zero length).
pub fn polyphony_rate(arr: &NoteArray, threshold: u32) -> f64 {
    let per_step = pitches_per_step(arr);
    if per_step.is_empty() {
        return f64::NAN;
    }
    let n = per_step.iter().filter(|&&c| c > threshold).count();
    n as f64 / per_step.len() as f64
}

/// Ratio of empty beats (beat = `resolution` steps); `NaN` if length is zero.
/// A note marks every beat it touches, inclusive of the beat its end lands on
/// (matching muspy).
pub fn empty_beat_rate(arr: &NoteArray) -> f64 {
    let length = arr.length();
    if length < 1 {
        return f64::NAN;
    }
    let res = arr.resolution.max(1) as u32;
    let n_beats = (length / res + 1) as usize;
    let mut is_empty = vec![true; n_beats];
    let mut count = 0usize;
    for note in &arr.notes {
        let start = (note.onset / res) as usize;
        let end = (((note.onset + note.duration) / res) as usize).min(n_beats - 1);
        for slot in is_empty[start..=end].iter_mut() {
            if *slot {
                *slot = false;
                count += 1;
            }
        }
    }
    1.0 - count as f64 / n_beats as f64
}

/// Scale mask for `root` and `mode` (`true` at pitch classes in the scale).
fn scale_mask(root: u8, major: bool) -> [bool; 12] {
    // C-rooted masks; rolled right by `root`.
    let c_scale: [bool; 12] = if major {
        [
            true, false, true, false, true, true, false, true, false, true, false, true,
        ]
    } else {
        [
            true, false, true, true, false, true, false, true, true, false, true, false,
        ]
    };
    let mut mask = [false; 12];
    for (i, m) in mask.iter_mut().enumerate() {
        // np.roll(c, root): result[i] = c[(i - root) % 12]
        *m = c_scale[((i as i32 - root as i32).rem_euclid(12)) as usize];
    }
    mask
}

/// Ratio of notes whose pitch class is in the given scale (`NaN` if no notes).
/// `mode`: `"major"` or `"minor"`.
pub fn pitch_in_scale_rate(arr: &NoteArray, root: u8, mode: &str) -> f64 {
    let major = match mode.to_ascii_lowercase().as_str() {
        "major" => true,
        "minor" => false,
        _ => return f64::NAN,
    };
    let mask = scale_mask(root % 12, major);
    if arr.notes.is_empty() {
        return f64::NAN;
    }
    let in_scale = arr
        .notes
        .iter()
        .filter(|n| mask[(n.pitch % 12) as usize])
        .count();
    in_scale as f64 / arr.notes.len() as f64
}

/// Largest pitch-in-scale rate over all 12 major and 12 minor scales
/// (`NaN` if no notes).
pub fn scale_consistency(arr: &NoteArray) -> f64 {
    if arr.notes.is_empty() {
        return f64::NAN;
    }
    let mut best = 0.0f64;
    for major in [true, false] {
        for root in 0..12u8 {
            let mask = scale_mask(root, major);
            let in_scale = arr
                .notes
                .iter()
                .filter(|n| mask[(n.pitch % 12) as usize])
                .count();
            let rate = in_scale as f64 / arr.notes.len() as f64;
            if rate > best {
                best = rate;
            }
        }
    }
    best
}

/// Groove consistency: `1 − mean Hamming distance between adjacent measures'
/// onset patterns`. `measure_resolution` is the number of steps per measure.
/// `NaN` if there are fewer than two measures.
pub fn groove_consistency(arr: &NoteArray, measure_resolution: u32) -> f64 {
    assert!(
        measure_resolution >= 1,
        "measure_resolution must be a positive integer"
    );
    let length = arr.length();
    let n_measures = (length / measure_resolution + 1) as usize;
    if n_measures < 2 {
        return f64::NAN;
    }
    let mr = measure_resolution as usize;
    let mut patterns = vec![false; n_measures * mr];
    for note in &arr.notes {
        let measure = (note.onset / measure_resolution) as usize;
        let position = (note.onset % measure_resolution) as usize;
        if measure < n_measures {
            patterns[measure * mr + position] = true;
        }
    }
    let mut hamming = 0usize;
    for m in 0..n_measures - 1 {
        for p in 0..mr {
            if patterns[m * mr + p] != patterns[(m + 1) * mr + p] {
                hamming += 1;
            }
        }
    }
    1.0 - hamming as f64 / (mr * (n_measures - 1)) as f64
}

#[cfg(test)]
mod tests {
    use super::super::note_array::NoteRow;
    use super::*;

    /// Build a note-array from `(onset, duration, pitch)` triples (velocity 64).
    fn na(resolution: u16, rows: &[(u32, u32, u8)]) -> NoteArray {
        let mut notes: Vec<NoteRow> = rows
            .iter()
            .map(|&(onset, duration, pitch)| NoteRow {
                onset,
                duration,
                pitch,
                velocity: 64,
            })
            .collect();
        notes.sort_by_key(|n| (n.onset, n.pitch, n.duration, n.velocity));
        NoteArray { resolution, notes }
    }

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "expected {b}, got {a}");
    }

    // A C-major scale: C D E F G A B as sequential quarters at resolution 4.
    fn c_major_scale() -> NoteArray {
        na(
            4,
            &[
                (0, 4, 60),
                (4, 4, 62),
                (8, 4, 64),
                (12, 4, 65),
                (16, 4, 67),
                (20, 4, 69),
                (24, 4, 71),
            ],
        )
    }

    #[test]
    fn test_counts_and_range() {
        let s = c_major_scale();
        assert_eq!(n_pitches_used(&s), 7);
        assert_eq!(n_pitch_classes_used(&s), 7);
        assert_eq!(pitch_range(&s), 11); // 71 - 60
    }

    #[test]
    fn test_pitch_class_histogram_and_entropy() {
        let s = c_major_scale();
        let hist = pitch_class_histogram(&s);
        // 7 classes each 1/7; others 0.
        for pc in [0u8, 2, 4, 5, 7, 9, 11] {
            approx(hist[pc as usize], 1.0 / 7.0);
        }
        approx(hist[1], 0.0);
        let total: f64 = hist.iter().sum();
        approx(total, 1.0);
        // Uniform over 7 classes → entropy log2(7).
        approx(pitch_class_entropy(&s), 7.0f64.log2());
        approx(pitch_entropy(&s), 7.0f64.log2());
    }

    #[test]
    fn test_scale_metrics() {
        let s = c_major_scale();
        approx(pitch_in_scale_rate(&s, 0, "major"), 1.0);
        approx(scale_consistency(&s), 1.0);
        // A note outside C major (C#) drops the C-major rate to 7/8.
        let mut with_sharp = s.clone();
        with_sharp.notes.push(NoteRow {
            onset: 28,
            duration: 4,
            pitch: 61,
            velocity: 64,
        });
        approx(pitch_in_scale_rate(&with_sharp, 0, "major"), 7.0 / 8.0);
    }

    #[test]
    fn test_polyphony() {
        // Sequential single notes → exactly one pitch on at every on-step.
        approx(polyphony(&c_major_scale()), 1.0);
        approx(polyphony_rate(&c_major_scale(), 2), 0.0);

        // A 3-note chord held for 4 steps: polyphony 3, all steps have >2.
        let chord = na(4, &[(0, 4, 60), (0, 4, 64), (0, 4, 67)]);
        approx(polyphony(&chord), 3.0);
        approx(polyphony_rate(&chord, 2), 1.0);
    }

    #[test]
    fn test_empty_beat_rate() {
        // Two quarter notes with a gap: C4 at [0,4), C4 at [16,20), resolution 4.
        // length 20, n_beats = 20/4 + 1 = 6; beats {0,1,4,5} touched → 2 empty.
        let arr = na(4, &[(0, 4, 60), (16, 4, 62)]);
        approx(empty_beat_rate(&arr), 2.0 / 6.0);
    }

    #[test]
    fn test_groove_consistency() {
        // C-major scale, measure_resolution 8 (2 quarters/measure).
        // Measures m0..m3 onset patterns: {0,4},{0,4},{0,4},{0}.
        // Hamming distances: 0,0,1 → groove = 1 - 1/(8*3).
        approx(groove_consistency(&c_major_scale(), 8), 1.0 - 1.0 / 24.0);
    }

    #[test]
    fn test_empty_array_edge_cases() {
        let empty = NoteArray {
            resolution: 4,
            notes: vec![],
        };
        assert_eq!(n_pitches_used(&empty), 0);
        assert_eq!(pitch_range(&empty), 0);
        assert!(pitch_class_histogram(&empty).iter().all(|&x| x == 0.0));
        assert!(pitch_entropy(&empty).is_nan());
        assert!(pitch_class_entropy(&empty).is_nan());
        assert!(polyphony(&empty).is_nan());
        assert!(scale_consistency(&empty).is_nan());
        assert!(empty_beat_rate(&empty).is_nan());
        // Single measure → groove undefined.
        assert!(groove_consistency(&na(4, &[(0, 4, 60)]), 8).is_nan());
    }
}
