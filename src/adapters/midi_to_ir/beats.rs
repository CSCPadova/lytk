//! Beat tracking for played MIDI whose tempo drifts from the file's
//! (rubato, a performance recorded without a click).
//!
//! Written from Dixon's description of BeatRoot (a beat tracker predicts
//! the next beat, takes the onset nearest the prediction inside a window,
//! and corrects its period by a fraction of the error), reduced to one
//! tracker started on the file's own beat: the file's tempo is assumed right
//! at the start and followed as it drifts.

/// Each beat's tick, when the playing drifts off the file's beat grid by
/// more than an eighth of a beat and the tracked beats put the onsets at
/// least twice as near 16ths or triplet eighths as the file's own do; `None`
/// otherwise (the
/// reader then leaves the times alone: notation, or steady playing).
/// `onsets` are sorted note-on ticks, `ppq` the file's ticks a beat.
// ponytail: one tracker from the file's tempo; a performance far from it
// (no click at all) needs tempo induction over inter-onset intervals first.
pub(super) fn drifting_beats(onsets: &[u64], ppq: u64) -> Option<Vec<u64>> {
    let (&last, p0) = (onsets.last()?, ppq as f64);
    let mut period = p0;
    let mut beats = vec![0u64];
    let mut beat = 0.0f64;
    // The onset nearest `at`, within a fifth of a beat.
    let near = |at: f64, period: f64| {
        let window = period / 5.0;
        let from = onsets.partition_point(|&t| (t as f64) < at - window);
        onsets[from..]
            .iter()
            .take_while(|&&t| (t as f64) <= at + window)
            .min_by(|&&a, &&b| (a as f64 - at).abs().total_cmp(&(b as f64 - at).abs()))
            .map(|&t| t as f64)
    };
    while beat <= last as f64 {
        let predicted = beat + period;
        beat = match near(predicted, period) {
            Some(t) => {
                let error = t - predicted;
                period = (period + error / 4.0).clamp(p0 / 2.0, p0 * 2.0);
                t
            }
            // Missed: back at the file's tempo ("a tempo" after a
            // ritardando) when the file's next beat is played too, or on as
            // predicted (a beat with no note of its own, an off-beat near
            // the file's beat, the last beat).
            None => match near(beat + p0, p0) {
                Some(t) if period != p0 && near(t + p0, p0).is_some() => {
                    period = p0;
                    t
                }
                _ => predicted,
            },
        };
        beats.push(beat.round() as u64);
    }
    let drift = beats
        .iter()
        .enumerate()
        .map(|(k, &b)| (b as f64 - k as f64 * p0).abs())
        .fold(0.0, f64::max);
    if drift <= p0 / 8.0 {
        return None;
    }
    // How far the onsets lie from 16ths or triplet eighths (swung eighths
    // among them), as written and as tracked.
    let off = |t: f64| {
        [p0 / 4.0, p0 / 3.0]
            .iter()
            .map(|&step| {
                let r = t.rem_euclid(step);
                r.min(step - r)
            })
            .fold(f64::MAX, f64::min)
    };
    let written: f64 = onsets.iter().map(|&t| off(t as f64)).sum();
    let tracked: f64 = onsets
        .iter()
        .map(|&t| off(onto_grid(t, &beats, ppq) as f64))
        .sum();
    (2.0 * tracked < written).then_some(beats)
}

/// A tick moved onto the beat grid: the `k`-th tracked beat becomes beat `k`,
/// linearly in between (and past the last beat at the last beat's length).
pub(super) fn onto_grid(t: u64, beats: &[u64], ppq: u64) -> u64 {
    let k = beats
        .partition_point(|&b| b <= t)
        .max(1)
        .min(beats.len() - 1);
    let (a, b) = (beats[k - 1], beats[k]);
    let within = (t as f64 - a as f64) / (b - a).max(1) as f64;
    ((k as f64 - 1.0 + within) * ppq as f64).round().max(0.0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Quarters slowing from 120 to about 96 bpm (480 ticks a beat).
    fn rubato() -> Vec<u64> {
        let mut t = 0u64;
        (0..8u64)
            .map(|i| {
                let on = t;
                t += 480 + i * 17;
                on
            })
            .collect()
    }

    #[test]
    fn a_slowing_performance_is_followed_beat_by_beat() {
        let onsets = rubato();
        let beats = drifting_beats(&onsets, 480).expect("it drifts");
        assert_eq!(&beats[..8], &onsets[..]);
        let back: Vec<u64> = onsets.iter().map(|&t| onto_grid(t, &beats, 480)).collect();
        assert_eq!(back, (0..8).map(|k| k * 480).collect::<Vec<_>>());
        // Half a beat after beat 3 stays half a beat after it.
        let mid = (onsets[3] + onsets[4]) / 2;
        assert_eq!(onto_grid(mid, &beats, 480), 3 * 480 + 240);
    }

    #[test]
    fn triplets_and_swing_played_rubato_are_followed() {
        // Beat k lasts 480 + 17k: eighth triplets, then swung pairs (2:1).
        let (mut t, mut trips, mut swung, mut starts) = (0u64, Vec::new(), Vec::new(), Vec::new());
        for k in 0..16u64 {
            let len = 480 + 17 * k;
            starts.push(t);
            trips.extend([t, t + len / 3, t + 2 * len / 3]);
            swung.extend([t, t + 2 * len / 3]);
            t += len;
        }
        for onsets in [trips, swung] {
            let beats = drifting_beats(&onsets, 480).expect("tracked");
            assert_eq!(&beats[..16], &starts[..]);
            // The last beat's notes stay in it.
            let last = onsets[onsets.len() - 1];
            assert!(onto_grid(last, &beats, 480) < 16 * 480, "{beats:?}");
        }
    }

    #[test]
    fn a_missing_beat_while_slow_is_no_a_tempo() {
        // A ritardando into a steady 600, and beat 13 has no onset (a tied
        // 16th): 6990 is no beat, the tempo stays slow.
        let onsets = [
            0u64, 480, 960, 1440, 1920, 2430, 2970, 3540, 4140, 4740, 5340, 5940, 6540, 6990, 7740,
            8340, 8940, 9540, 10140, 10740, 11340,
        ];
        let beats = drifting_beats(&onsets, 480).expect("it drifts");
        let at = |t: u64| onto_grid(t, &beats, 480);
        assert_eq!(at(7740), 14 * 480, "{beats:?}");
        assert_eq!(at(11340), 20 * 480);
        assert!(at(6990) > 12 * 480 && at(6990) < 13 * 480);
    }

    #[test]
    fn steady_playing_keeps_the_file_grid() {
        // Sixteenths ±20 ticks around a steady beat, and syncopations.
        let jitter = [0i64, 12, -20, 7, 19, -15, 3, -9, 20, -4, 11, -18];
        let onsets: Vec<u64> = (0..48u64)
            .map(|i| (i as i64 * 120 + if i == 0 { 0 } else { jitter[i as usize % 12] }) as u64)
            .collect();
        assert_eq!(drifting_beats(&onsets, 480), None);
        let synco = [0u64, 360, 720, 1080, 1440, 1920];
        assert_eq!(drifting_beats(&synco, 480), None);
        // Notation with no note on the beats, only a 32nd before each: the
        // tracker would lock onto them, but they are on the grid already.
        let before: Vec<u64> = (1..16u64).map(|k| k * 480 - 60).collect();
        assert_eq!(drifting_beats(&before, 480), None);
    }
}
