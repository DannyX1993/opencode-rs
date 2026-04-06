//! Typed ID newtypes wrapping [`uuid::Uuid`].
//!
//! Each domain entity gets its own newtype so the compiler prevents accidental
//! mixing of e.g. a `SessionId` where a `MessageId` is expected.

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

macro_rules! id_newtype {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[repr(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Generate a new random ID.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Return the underlying [`Uuid`].
            #[must_use]
            #[inline]
            pub fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<Uuid> for $name {
            fn from(u: Uuid) -> Self {
                Self(u)
            }
        }

        impl From<$name> for Uuid {
            fn from(id: $name) -> Uuid {
                id.0
            }
        }

        impl std::str::FromStr for $name {
            type Err = uuid::Error;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(Self(s.parse()?))
            }
        }
    };
}

id_newtype!(SessionId, "Strongly-typed session identifier.");
id_newtype!(MessageId, "Strongly-typed message identifier.");
id_newtype!(PartId, "Strongly-typed message-part identifier.");
id_newtype!(ProjectId, "Strongly-typed project identifier.");
id_newtype!(TodoId, "Strongly-typed todo-item identifier.");
id_newtype!(WorkspaceId, "Strongly-typed workspace identifier.");
id_newtype!(AccountId, "Strongly-typed account identifier.");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_distinct_types() {
        let s = SessionId::new();
        let m = MessageId::new();
        // Confirm round-trips
        let s2: SessionId = s.as_uuid().into();
        let m2: MessageId = m.as_uuid().into();
        assert_eq!(s, s2);
        assert_eq!(m, m2);
        // Display
        assert!(!s.to_string().is_empty());
        assert!(!m.to_string().is_empty());
    }

    #[test]
    fn id_from_str() {
        let id = SessionId::new();
        let s = id.to_string();
        let parsed: SessionId = s.parse().expect("round-trip parse");
        assert_eq!(id, parsed);
    }

    #[test]
    fn all_id_types_construct() {
        let _ = SessionId::new();
        let _ = MessageId::new();
        let _ = PartId::new();
        let _ = ProjectId::new();
        let _ = TodoId::new();
        let _ = WorkspaceId::new();
        let _ = AccountId::new();
    }
}
