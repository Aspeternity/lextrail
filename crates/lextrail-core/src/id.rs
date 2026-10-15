//! Record and device identifiers (ARCHITECTURE.md 6.1).

use uuid::Uuid;

/// Parses a UUID only in the canonical stored form: lowercase, hyphenated, 36 characters.
/// Uppercase, braced, URN and unhyphenated forms are rejected rather than normalized.
pub fn parse_canonical_uuid(s: &str) -> Option<Uuid> {
    let uuid = Uuid::try_parse(s).ok()?;
    (uuid.hyphenated().to_string() == s).then_some(uuid)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "019a1b2c-3d4e-7abc-8def-0123456789ab";

    #[test]
    fn accepts_canonical_form() {
        assert_eq!(parse_canonical_uuid(ID).unwrap().to_string(), ID);
    }

    #[test]
    fn rejects_other_forms() {
        for s in [
            "019A1B2C-3D4E-7ABC-8DEF-0123456789AB",
            "019a1b2c3d4e7abc8def0123456789ab",
            "{019a1b2c-3d4e-7abc-8def-0123456789ab}",
            "urn:uuid:019a1b2c-3d4e-7abc-8def-0123456789ab",
            "",
        ] {
            assert_eq!(parse_canonical_uuid(s), None, "{s}");
        }
    }
}
