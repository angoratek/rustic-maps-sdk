//! Value coercion, comparison and collection helpers shared by the expression
//! evaluator and the filter normaliser.
//!
//! Coercion follows the specification rather than Rust's `PartialEq`: numbers
//! compare numerically, strings lexically, and a mixed number/string
//! comparison is false unless a conversion operator produced one of the
//! values.

use std::cmp::Ordering;

use crate::color::Color;
use crate::types::Value;

/// Formats a number for `to-string`, `concat` and label text.
///
/// Integral values print without a fractional part (`1`, not `1.0`); every
/// other finite value uses Rust's shortest round-tripping representation.
pub fn number_to_string(number: f64) -> String {
    if number.is_finite() && number.fract() == 0.0 && number.abs() < MAXIMUM_INTEGRAL_DISPLAY {
        format!("{number:.0}")
    } else {
        number.to_string()
    }
}

/// The largest magnitude displayed as an integer by [`number_to_string`].
const MAXIMUM_INTEGRAL_DISPLAY: f64 = 1.0e15;

/// Converts a value to a string following the specification's `to-string`
/// rules.
pub fn value_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Boolean(boolean) => boolean.to_string(),
        Value::Number(number) => number_to_string(*number),
        Value::String(string) => string.clone(),
        Value::Color(color) => color.to_css_string(),
        Value::Array(_) | Value::Object(_) => value.to_json().to_string(),
    }
}

/// Converts a value to a number, or `None` when the value has no numeric
/// interpretation.
pub fn value_to_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => Some(*number),
        Value::Boolean(boolean) => Some(if *boolean { 1.0 } else { 0.0 }),
        Value::String(string) => string.trim().parse::<f64>().ok(),
        Value::Null | Value::Color(_) | Value::Array(_) | Value::Object(_) => None,
    }
}

/// Converts a value to a colour, or `None` when it is not a colour and does
/// not parse as one.
pub fn value_to_color(value: &Value) -> Option<Color> {
    match value {
        Value::Color(color) => Some(*color),
        Value::String(string) => Color::from_css(string).ok(),
        Value::Array(array) => {
            let channels: Option<Vec<f64>> = array.iter().map(Value::as_number).collect();
            let channels = channels?;
            match channels.as_slice() {
                [red, green, blue] => Some(Color::from_rgb_numbers(*red, *green, *blue, 1.0)),
                [red, green, blue, alpha] => {
                    Some(Color::from_rgb_numbers(*red, *green, *blue, *alpha))
                }
                _ => None,
            }
        }
        Value::Null | Value::Boolean(_) | Value::Number(_) | Value::Object(_) => None,
    }
}

/// Whether two values are equal under the specification's comparison rules.
pub fn values_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Null, Value::Null) => true,
        (Value::Boolean(first), Value::Boolean(second)) => first == second,
        (Value::Number(first), Value::Number(second)) => first == second,
        (Value::String(first), Value::String(second)) => first == second,
        (Value::Color(first), Value::Color(second)) => first == second,
        (Value::Array(first), Value::Array(second)) => {
            first.len() == second.len()
                && first
                    .iter()
                    .zip(second)
                    .all(|(first_entry, second_entry)| values_equal(first_entry, second_entry))
        }
        (Value::Object(first), Value::Object(second)) => {
            first.len() == second.len()
                && first.iter().all(|(key, first_value)| {
                    second
                        .get(key)
                        .is_some_and(|second_value| values_equal(first_value, second_value))
                })
        }
        (Value::Color(color), Value::String(text)) | (Value::String(text), Value::Color(color)) => {
            Color::from_css(text).is_ok_and(|parsed| parsed == *color)
        }
        _ => false,
    }
}

/// Orders two values when they share a comparable type.
pub fn values_order(left: &Value, right: &Value) -> Option<Ordering> {
    match (left, right) {
        (Value::Number(first), Value::Number(second)) => first.partial_cmp(second),
        (Value::String(first), Value::String(second)) => Some(first.cmp(second)),
        (Value::Boolean(first), Value::Boolean(second)) => Some(first.cmp(second)),
        _ => None,
    }
}

/// The number of characters in a string or entries in an array.
pub fn value_length(value: &Value) -> Option<usize> {
    match value {
        Value::String(string) => Some(string.chars().count()),
        Value::Array(array) => Some(array.len()),
        _ => None,
    }
}

/// The entry at `index` in a string or array; a negative index counts from
/// the end, as the specification's `at` operator specifies.
pub fn value_at(value: &Value, index: i64) -> Option<Value> {
    match value {
        Value::String(string) => {
            let characters: Vec<char> = string.chars().collect();
            let position = resolve_index(index, characters.len())?;
            characters
                .get(position)
                .map(|character| Value::String(character.to_string()))
        }
        Value::Array(array) => {
            let position = resolve_index(index, array.len())?;
            array.get(position).cloned()
        }
        _ => None,
    }
}

/// A slice of a string or array between `start` and an optional `end`.
pub fn value_slice(value: &Value, start: i64, end: Option<i64>) -> Option<Value> {
    match value {
        Value::String(string) => {
            let characters: Vec<char> = string.chars().collect();
            let lower = resolve_clamped_index(start, characters.len());
            let upper = end.map_or(characters.len(), |end| {
                resolve_clamped_index(end, characters.len())
            });
            if lower >= upper {
                return Some(Value::String(String::new()));
            }
            Some(Value::String(characters[lower..upper].iter().collect()))
        }
        Value::Array(array) => {
            let lower = resolve_clamped_index(start, array.len());
            let upper = end.map_or(array.len(), |end| resolve_clamped_index(end, array.len()));
            if lower >= upper {
                return Some(Value::Array(Vec::new()));
            }
            Some(Value::Array(array[lower..upper].to_vec()))
        }
        _ => None,
    }
}

/// Whether a string contains a substring or an array contains a value.
pub fn value_contains(haystack: &Value, needle: &Value) -> Option<bool> {
    match (haystack, needle) {
        (Value::String(haystack), Value::String(needle)) => Some(haystack.contains(needle)),
        (Value::Array(array), needle) => {
            Some(array.iter().any(|entry| values_equal(entry, needle)))
        }
        _ => None,
    }
}

/// The first position of `needle` in a string or array.
pub fn value_index_of(haystack: &Value, needle: &Value) -> Option<i64> {
    match (haystack, needle) {
        (Value::String(haystack), Value::String(needle)) => haystack
            .find(needle)
            .map(|byte_index| haystack[..byte_index].chars().count() as i64),
        (Value::Array(array), needle) => array
            .iter()
            .position(|entry| values_equal(entry, needle))
            .map(|position| position as i64),
        _ => None,
    }
}

fn resolve_index(index: i64, length: usize) -> Option<usize> {
    if index < 0 {
        let from_end = index.checked_neg()?;
        let position = (length as i64).checked_sub(from_end)?;
        usize::try_from(position)
            .ok()
            .filter(|position| *position < length)
    } else {
        usize::try_from(index)
            .ok()
            .filter(|position| *position < length)
    }
}

fn resolve_clamped_index(index: i64, length: usize) -> usize {
    if index < 0 {
        let from_end = index.saturating_neg() as usize;
        length.saturating_sub(from_end)
    } else {
        (index as usize).min(length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_types_never_compare_equal() {
        assert!(!values_equal(
            &Value::Number(1.0),
            &Value::String("1".into())
        ));
        assert!(!values_equal(&Value::Boolean(true), &Value::Number(1.0)));
        assert!(values_equal(&Value::Null, &Value::Null));
    }

    #[test]
    fn collections_compare_deeply() {
        let left = Value::Array(vec![Value::Number(1.0), Value::String("a".into())]);
        let right = Value::Array(vec![Value::Number(1.0), Value::String("a".into())]);
        assert!(values_equal(&left, &right));
        assert!(!values_equal(
            &left,
            &Value::Array(vec![Value::Number(1.0)])
        ));
    }

    #[test]
    fn negative_indices_count_from_the_end() {
        let value = Value::String("map".into());
        assert_eq!(value_at(&value, -1), Some(Value::String("p".into())));
        assert_eq!(value_at(&value, 5), None);
        assert_eq!(
            value_slice(&value, 1, None),
            Some(Value::String("ap".into()))
        );
    }

    #[test]
    fn numbers_display_without_a_fractional_part() {
        assert_eq!(number_to_string(1.0), "1");
        assert_eq!(number_to_string(1.5), "1.5");
    }
}
