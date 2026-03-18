//! Utility functions for LilyPond emission.

use crate::ir::Part;

/// Sanitise a part id/name into a valid LilyPond variable name.
pub(super) fn part_var_name(part: &Part) -> String {
    let raw = if !part.part_id.is_empty() {
        &part.part_id
    } else if !part.name.is_empty() {
        &part.name
    } else {
        "part"
    };

    // LilyPond variable names must be all-letter (no digits) so the
    // tree-sitter grammar produces an `assignment_lhs` node. Convert any
    // trailing numeric index to an alphabetic suffix.
    let alpha: String = raw.chars().filter(|c| c.is_alphabetic()).collect();
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();

    let base = if alpha.is_empty() { "part".to_string() } else { alpha };
    let mut name = camel_to_lower(&base);

    if !digits.is_empty() {
        if let Ok(n) = digits.parse::<usize>() {
            name.push_str(&index_to_alpha(n));
        }
    }

    name
}

/// Convert a 1-based index to alphabetic suffix: 1->A, 2->B, 26->Z, 27->AA
pub(super) fn index_to_alpha(n: usize) -> String {
    if n == 0 {
        return "A".to_string();
    }
    let mut result = String::new();
    let mut val = n;
    while val > 0 {
        val -= 1;
        result.insert(0, (b'A' + (val % 26) as u8) as char);
        val /= 26;
    }
    result
}

/// Lowercase the first character of single-word names (e.g. "P" -> "p").
pub(super) fn camel_to_lower(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// Simple Roman numeral for staff numbering (1-5).
pub(super) fn roman(n: u8) -> &'static str {
    match n {
        1 => "I",
        2 => "II",
        3 => "III",
        4 => "IV",
        5 => "V",
        _ => "X",
    }
}
