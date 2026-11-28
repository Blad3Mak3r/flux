//! Helpers for `skip_serializing_if` on serde fields, shared by the project types.

/// Whether a flag is off, so it is left out of the file.
pub(crate) fn is_false(value: &bool) -> bool {
    !*value
}

/// Whether a value is its type's default (zero, empty), so it is left out of the file.
pub(crate) fn is_zero<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// The default of a setting at its full level (1.0).
pub(crate) fn full() -> f32 {
    1.0
}

/// Whether a setting is at its full level, so it is left out of the file.
pub(crate) fn is_full(value: &f32) -> bool {
    *value == 1.0
}
