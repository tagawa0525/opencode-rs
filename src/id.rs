//! ID generation utilities using ULID for time-ordered unique identifiers.
//!
//! This module provides ID generation similar to opencode-ts's Identifier module,
//! supporting both ascending (chronological) and descending (reverse chronological) IDs.

use ulid::Ulid;

/// ID prefix types for different entities
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdPrefix {
    Session,
    Message,
    Part,
}

impl IdPrefix {
    pub fn as_str(&self) -> &'static str {
        match self {
            IdPrefix::Session => "ses",
            IdPrefix::Message => "msg",
            IdPrefix::Part => "prt",
        }
    }
}

/// Generate an ascending (chronologically ordered) ID
pub fn ascending(prefix: IdPrefix) -> String {
    let ulid = Ulid::new();
    format!("{}_{}", prefix.as_str(), ulid.to_string().to_lowercase())
}

/// Generate a descending (reverse chronologically ordered) ID
/// This is useful for listing items where newest should appear first
pub fn descending(prefix: IdPrefix) -> String {
    let ulid = Ulid::new();
    // Invert the timestamp portion to get descending order
    let inverted = invert_ulid(&ulid);
    format!("{}_{}", prefix.as_str(), inverted)
}

/// Invert a ULID for descending order
fn invert_ulid(ulid: &Ulid) -> String {
    let bytes = ulid.to_bytes();
    let mut inverted = [0u8; 16];

    // Invert all bytes
    for (i, &b) in bytes.iter().enumerate() {
        inverted[i] = !b;
    }

    // Convert to base32-like string (simplified)
    let ulid_inverted = Ulid::from_bytes(inverted);
    ulid_inverted.to_string().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ascending_id() {
        let id1 = ascending(IdPrefix::Session);
        std::thread::sleep(std::time::Duration::from_millis(1));
        let id2 = ascending(IdPrefix::Session);

        assert!(id1.starts_with("ses_"));
        assert!(id2.starts_with("ses_"));
        assert!(id1 < id2); // IDs should be chronologically ordered
    }

    #[test]
    fn test_descending_id() {
        let id1 = descending(IdPrefix::Session);
        std::thread::sleep(std::time::Duration::from_millis(1));
        let id2 = descending(IdPrefix::Session);

        assert!(id1.starts_with("ses_"));
        assert!(id2.starts_with("ses_"));
        assert!(id1 > id2); // IDs should be reverse chronologically ordered
    }
}
