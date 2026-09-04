//! The two enum shapes this crate distinguishes, as declarative macros.
//!
//! OICP 2.3 has no formal notion of an extensible enum — every enumerated type in the
//! specification is written as a closed list. In practice that is not how the wire behaves:
//!
//! * Hubject edits the OICP 2.3 documents **in place**, with no version bump. `Plug`,
//!   `ValueAddedService` and `PaymentOption` have each grown values this way.
//! * A hub forwards records between two parties. Discarding a `Plug` value this crate has never
//!   heard of turns a faithful forward into silent data loss.
//!
//! So the default in this crate is [`oicp_open_enum!`](crate::oicp_open_enum): every documented
//! value becomes a variant, and anything else is **kept** in a `Custom(String)` variant that
//! re-serialises byte-identically. [`Validate`](crate::types::Validate) reports the unknown value
//! as [`ViolationCode::PatternMismatch`](crate::types::ViolationCode::PatternMismatch) — so it is
//! visible in a conformance report — but decoding never fails and the value is never rewritten.
//!
//! [`oicp_enum!`](crate::oicp_enum) generates the strict shape, for the handful of types where an
//! unknown value genuinely is a protocol error rather than an extension: `ActionType`, where
//! guessing would risk deleting a fleet, and `AuthorizationStatus`, where "neither Authorized nor
//! NotAuthorized" has no safe interpretation.
//!
//! Both macros are exported, so a party defining its own extension types can use them.

/// Why a string is not a member of a closed OICP enum.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownVariant {
    enum_name: &'static str,
    value: String,
    allowed: &'static [&'static str],
}

impl UnknownVariant {
    /// Creates an error for `value`, which is not one of `allowed`.
    #[must_use]
    pub fn new(enum_name: &'static str, value: impl Into<String>, allowed: &'static [&'static str]) -> Self {
        Self { enum_name, value: value.into(), allowed }
    }

    /// The value that was not recognised.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// The name of the enum that rejected it.
    #[must_use]
    pub const fn enum_name(&self) -> &'static str {
        self.enum_name
    }

    /// Every value the enum does accept.
    #[must_use]
    pub const fn allowed(&self) -> &'static [&'static str] {
        self.allowed
    }
}

impl core::fmt::Display for UnknownVariant {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?} is not a valid {}; expected one of ", self.value, self.enum_name)?;
        for (i, a) in self.allowed.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{a}")?;
        }
        Ok(())
    }
}

impl std::error::Error for UnknownVariant {}

/// Defines a **closed** OICP enum: a fixed set of strings, where anything else is an error.
///
/// Reserved for the types where an unrecognised value has no safe interpretation. See the
/// [open-enum rule](crate::types#open-enums-by-default) for why that is the minority.
///
/// ```
/// use oicp_kit::oicp_enum;
///
/// oicp_enum! {
///     /// Whether the user may charge.
///     pub enum AuthorizationStatus {
///         /// User is authorized.
///         Authorized = "Authorized",
///         /// User is not authorized.
///         NotAuthorized = "NotAuthorized",
///     }
/// }
///
/// assert_eq!(AuthorizationStatus::Authorized.as_str(), "Authorized");
/// assert!("Maybe".parse::<AuthorizationStatus>().is_err());
/// ```
///
/// Attributes that document the enum — `#[cfg_attr(docsrs, doc(cfg(…)))]` above all — go **inside**
/// the invocation, on the `pub enum` line, so they land on the item this expands to.
#[macro_export]
macro_rules! oicp_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$vmeta:meta])*
                $variant:ident = $wire:literal
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        $vis enum $name {
            $(
                $(#[$vmeta])*
                #[doc = concat!("\n\nWire value: `", $wire, "`")]
                $variant,
            )*
        }

        #[allow(dead_code)]
        impl $name {
            /// Every value this enum accepts, in declaration order.
            pub const ALL: &'static [Self] = &[ $( Self::$variant ),* ];
            /// Every wire value this enum accepts, in declaration order.
            pub const ALL_WIRE: &'static [&'static str] = &[ $( $wire ),* ];

            /// The value as it appears on the wire.
            #[must_use]
            pub const fn as_str(&self) -> &'static str {
                match self { $( Self::$variant => $wire, )* }
            }

            /// Parses a wire value, ignoring ASCII case.
            ///
            /// OICP enum values are case-sensitive, so this is only for peers known to get the
            /// case wrong; [`FromStr`](core::str::FromStr) is the strict version.
            #[must_use]
            pub fn from_str_ignore_case(s: &str) -> Option<Self> {
                $( if s.eq_ignore_ascii_case($wire) { return Some(Self::$variant); } )*
                None
            }
        }

        impl core::fmt::Display for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl core::str::FromStr for $name {
            type Err = $crate::types::UnknownVariant;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $( $wire => Ok(Self::$variant), )*
                    other => Err($crate::types::UnknownVariant::new(
                        stringify!($name), other, Self::ALL_WIRE,
                    )),
                }
            }
        }

        impl $crate::types::Validate for $name {
            fn validate_in(&self, _v: &mut $crate::types::Validator) {}
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.as_str())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                struct V;
                impl serde::de::Visitor<'_> for V {
                    type Value = $name;
                    fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                        write!(f, "one of the {} values of {}", $name::ALL_WIRE.len(), stringify!($name))
                    }
                    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<$name, E> {
                        <$name as core::str::FromStr>::from_str(v).map_err(E::custom)
                    }
                }
                d.deserialize_str(V)
            }
        }

        #[cfg(feature = "schema")]
        impl schemars::JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> { stringify!($name).into() }
            fn json_schema(_g: &mut schemars::SchemaGenerator) -> schemars::Schema {
                schemars::json_schema!({ "type": "string", "enum": Self::ALL_WIRE })
            }
        }
    };
}

/// Defines an OICP enum that keeps values it does not know.
///
/// The documented values become variants; anything else lands in `Custom(String)` and is written
/// back verbatim. This is the default shape in this crate — see the
/// [open-enum rule](crate::types#open-enums-by-default).
///
/// ```
/// use oicp_kit::oicp_open_enum;
///
/// oicp_open_enum! {
///     /// The plug type of a charging facility.
///     pub enum Plug {
///         /// IEC 62196-1 type 2.
///         Type2Outlet = "Type 2 Outlet",
///         /// DC CHAdeMO connector.
///         ChaDeMo = "CHAdeMO",
///     }
/// }
///
/// // A value Hubject added after this crate was written survives untouched.
/// let future: Plug = "MCS".parse().unwrap();
/// assert!(!future.is_known());
/// assert_eq!(future.as_str(), "MCS");
/// assert_eq!(serde_json::to_string(&future).unwrap(), r#""MCS""#);
/// ```
#[macro_export]
macro_rules! oicp_open_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$vmeta:meta])*
                $variant:ident = $wire:literal
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        ///
        /// A value OICP 2.3 does not document is **kept** rather than rejected: it lands in
        /// `Custom` and re-serialises byte-identically. [`Validate`](crate::types::Validate)
        /// reports it, so a conformance run still sees it.
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        $vis enum $name {
            $(
                $(#[$vmeta])*
                #[doc = concat!("\n\nWire value: `", $wire, "`")]
                $variant,
            )*
            /// A value this crate does not know, preserved exactly as it arrived.
            Custom(String),
        }

        #[allow(dead_code)]
        impl $name {
            /// Every documented value, in declaration order.
            pub const ALL: &'static [Self] = &[ $( Self::$variant ),* ];
            /// Every documented wire value, in declaration order.
            pub const ALL_WIRE: &'static [&'static str] = &[ $( $wire ),* ];

            /// The value as it appears on the wire.
            #[must_use]
            pub fn as_str(&self) -> &str {
                match self {
                    $( Self::$variant => $wire, )*
                    Self::Custom(s) => s.as_str(),
                }
            }

            /// Whether this is a value the specification documents.
            #[must_use]
            pub fn is_known(&self) -> bool {
                !matches!(self, Self::Custom(_))
            }

            /// Parses a wire value, ignoring ASCII case for the documented values.
            ///
            /// A value that matches nothing becomes `Custom` with its original text, so this
            /// never fails.
            #[must_use]
            pub fn from_str_ignore_case(s: &str) -> Self {
                $( if s.eq_ignore_ascii_case($wire) { return Self::$variant; } )*
                Self::Custom(s.to_owned())
            }
        }

        impl core::fmt::Display for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl core::str::FromStr for $name {
            type Err = core::convert::Infallible;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(match s {
                    $( $wire => Self::$variant, )*
                    other => Self::Custom(other.to_owned()),
                })
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                match s {
                    $( $wire => Self::$variant, )*
                    other => Self::Custom(other.to_owned()),
                }
            }
        }

        impl $crate::types::Validate for $name {
            fn validate_in(&self, v: &mut $crate::types::Validator) {
                if let Self::Custom(value) = self {
                    v.report(
                        $crate::types::ViolationCode::PatternMismatch,
                        format!(
                            "{value:?} is not one of the {} values OICP 2.3 documents for {}; \
                             it is preserved, but a peer that has not agreed on it will not \
                             understand it",
                            Self::ALL_WIRE.len(), stringify!($name),
                        ),
                    );
                }
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.as_str())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                struct V;
                impl serde::de::Visitor<'_> for V {
                    type Value = $name;
                    fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                        write!(f, "a {} value", stringify!($name))
                    }
                    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<$name, E> {
                        Ok($name::from(v))
                    }
                }
                d.deserialize_str(V)
            }
        }

        #[cfg(feature = "schema")]
        impl schemars::JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> { stringify!($name).into() }
            fn json_schema(_g: &mut schemars::SchemaGenerator) -> schemars::Schema {
                // `enum` would make a documented value the only legal one; OICP grows these
                // lists in place, so the schema documents the known values without closing them.
                schemars::json_schema!({ "type": "string", "examples": Self::ALL_WIRE })
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use crate::types::Validate;

    oicp_open_enum! {
        /// Test enum.
        pub enum Sample {
            /// A.
            Alpha = "Alpha",
            /// B.
            Beta = "Beta",
        }
    }

    oicp_enum! {
        /// Closed test enum.
        pub enum Closed {
            /// A.
            Alpha = "Alpha",
        }
    }

    #[test]
    fn open_enum_preserves_unknown_values_byte_for_byte() {
        let json = r#""SomethingHubjectAddedLater""#;
        let value: Sample = serde_json::from_str(json).unwrap();
        assert!(!value.is_known());
        assert_eq!(serde_json::to_string(&value).unwrap(), json);
        // Preserved, but still reported.
        assert_eq!(value.validate().unwrap_err().len(), 1);
    }

    #[test]
    fn open_enum_known_values_validate_clean() {
        let value: Sample = serde_json::from_str(r#""Beta""#).unwrap();
        assert_eq!(value, Sample::Beta);
        assert!(value.validate().is_ok());
    }

    #[test]
    fn closed_enum_rejects_unknown_values() {
        assert!(serde_json::from_str::<Closed>(r#""Beta""#).is_err());
    }

    #[test]
    fn case_insensitive_parsing_is_opt_in() {
        assert_eq!(Sample::from_str_ignore_case("alpha"), Sample::Alpha);
        assert_eq!(Sample::from("alpha"), Sample::Custom("alpha".into()));
    }
}
