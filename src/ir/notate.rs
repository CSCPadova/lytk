//! Written note values for a length: `notate(5/16)` is a quarter tied to a
//! sixteenth. Used where music is cut at bar lines (the Layer-1 lowering, the
//! MIDI reader) and a piece has to be spelled as notes.

use super::duration::{Duration, Frac};

/// The shortest value spelled (a 128th); anything shorter is left as it is.
fn shortest() -> Frac {
    Frac::new(1, 128)
}

/// Spell `len` (whole notes, sounding) as written values to tie together,
/// longest first, each at most single-dotted. Inside a tuplet (`ratio` =
/// actual:normal, e.g. 3:2) the values are the written ones and carry the
/// ratio. A remainder shorter than a 128th becomes one value of its own.
pub(crate) fn notate(len: Frac, ratio: Option<(u8, u8)>) -> Vec<Duration> {
    let (actual, normal) = ratio.unwrap_or((1, 1));
    let scale = Frac::new(actual.max(1) as i64, normal.max(1) as i64);
    let mut rest = len * scale; // written length
    let mut out = Vec::new();
    let zero = Frac::from_integer(0);
    while rest > zero {
        let mut base = Frac::from_integer(2); // breve
        while base > rest && base >= shortest() {
            base /= Frac::from_integer(2);
        }
        let mut d = if base < shortest() {
            // Nothing spellable fits: keep the remainder whole.
            let d = Duration::new(rest);
            rest = zero;
            d
        } else if base * Frac::new(3, 2) <= rest {
            rest -= base * Frac::new(3, 2);
            Duration::dotted(base, 1)
        } else {
            rest -= base;
            Duration::new(base)
        };
        if ratio.is_some() {
            d.tuplet_actual = actual;
            d.tuplet_normal = normal;
        }
        out.push(d);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(len: Frac, ratio: Option<(u8, u8)>) -> Vec<(Frac, u8)> {
        notate(len, ratio)
            .iter()
            .map(|d| (d.base, d.dots))
            .collect()
    }

    #[test]
    fn spells_lengths_as_tied_values() {
        let q = |n, d| Frac::new(n, d);
        assert_eq!(values(q(1, 4), None), vec![(q(1, 4), 0)]);
        assert_eq!(values(q(3, 8), None), vec![(q(1, 4), 1)]);
        assert_eq!(values(q(5, 16), None), vec![(q(1, 4), 0), (q(1, 16), 0)]);
        assert_eq!(values(q(7, 8), None), vec![(q(1, 2), 1), (q(1, 8), 0)]);
        assert_eq!(values(q(5, 4), None), vec![(q(1, 1), 0), (q(1, 4), 0)]);
        // A triplet eighth (1/12 sounding) is a written eighth, 3:2.
        let t = notate(q(1, 12), Some((3, 2)));
        assert_eq!(
            (t[0].base, t[0].tuplet_actual, t[0].tuplet_normal),
            (q(1, 8), 3, 2)
        );
        assert_eq!(t[0].actual_duration(), q(1, 12));
        let total: Frac = notate(q(13, 32), None)
            .iter()
            .map(|d| d.actual_duration())
            .sum();
        assert_eq!(total, q(13, 32));
    }
}
