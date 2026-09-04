//! `strict_builder!` — makes a generated builder's `build()` the place conformance is enforced.
//!
//! # The rule this implements
//!
//! > Parse permissively, validate explicitly, **construct strictly**.
//!
//! Deserialisation accepts what a peer sent. [`Validate::validate`](super::Validate::validate)
//! reports what is wrong with it. But an object *this crate builds* should never be out of spec in
//! the first place — a partner should not be able to publish a 60-character city name to the
//! roaming network by accident and only find out when Hubject rejects the push, hours later, for
//! the whole batch.
//!
//! Field-level types stay permissive so a builder can take a `&str` (see [`Text`](super::Text)),
//! and the check happens once, on the finished object:
//!
//! * `build()` validates and returns `Result<T, Violations>`.
//! * `build_unchecked()` skips the check, for tests and for re-emitting a peer's payload as it
//!   arrived.
//!
//! The name of the second one is the documentation.

/// Gives a `bon`-generated builder a validating `build()` alongside its `build_unchecked()`.
///
/// The type must carry `#[builder(finish_fn = build_unchecked)]`. Arguments are the type, its
/// generated builder, and the generated state module — `bon` derives all three names from the
/// type, but Rust macros cannot concatenate identifiers, so they are spelled out.
macro_rules! strict_builder {
    ($ty:ident, $builder:ident, $module:ident) => {
        impl<S: $module::IsComplete> $builder<S> {
            #[doc = concat!("Finishes the `", stringify!($ty), "`, checking it against the specification.")]
            ///
            /// # Errors
            ///
            /// Returns every [`Violation`](crate::types::Violation) found, so a value that is
            /// already out of spec never reaches the wire. Call `build_unchecked()` to skip the
            /// check — for a test fixture, or to re-emit a peer's payload verbatim.
            pub fn build(self) -> Result<$ty, $crate::types::Violations> {
                let value = self.build_unchecked();
                $crate::types::Validate::validate(&value)?;
                Ok(value)
            }
        }
    };
}

pub(crate) use strict_builder;
