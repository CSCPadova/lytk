//! Serde helpers: the IR's JSON leaves out every value that is its
//! default, and reads a value left out as that default.

/// Whether a value is its type's default (empty, `None`, `false`, 0).
pub(crate) fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// 1: the default voice, staff, verse, beam level and tuplet ratio.
pub(crate) fn one<T: From<u8>>() -> T {
    T::from(1)
}

pub(crate) fn is_one<T: From<u8> + PartialEq>(value: &T) -> bool {
    *value == T::from(1)
}

/// A fraction read in lowest terms (`[2, 8]` is 1/4), as all arithmetic
/// on the IR's fractions expects.
pub(crate) fn reduced<'de, D, T>(deserializer: D) -> Result<num::rational::Ratio<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Clone + num::Integer + serde::Deserialize<'de>,
{
    let r = <num::rational::Ratio<T> as serde::Deserialize>::deserialize(deserializer)?;
    // The deserializer refuses a zero denominator.
    Ok(num::rational::Ratio::new(
        r.numer().clone(),
        r.denom().clone(),
    ))
}

pub(crate) fn yes() -> bool {
    true
}

pub(crate) fn is_yes(value: &bool) -> bool {
    *value
}
