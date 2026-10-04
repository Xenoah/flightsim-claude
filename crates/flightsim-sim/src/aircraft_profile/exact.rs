//! Numeric tokens reach Rust's correctly rounded parser without serde_json's
//! default f64 conversion. Never route profile input through a Value or enum buffer.
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::value::RawValue;
use std::{fmt, marker::PhantomData};

/// Maximum bytes in one JSON numeric token, checked before numeric conversion.
pub const MAX_NUMBER_BYTES: usize = 128;

/// Finite binary64 at the external profile boundary. The private field prevents
/// nonfinite construction. Physical units are stated by the enclosing field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExactF64(f64);

impl ExactF64 {
    /// Construct from existing finite binary64 bits (including negative zero).
    pub fn new(value: f64) -> Result<Self, super::ProfileError> {
        if value.is_finite() {
            Ok(Self(value))
        } else {
            Err(super::ProfileError("profile numbers must be finite".into()))
        }
    }

    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

impl Serialize for ExactF64 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(self.0)
    }
}

impl<'de> Deserialize<'de> for ExactF64 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Borrow directly from the bounded input. Box<RawValue>, Value,
        // from_value, untagged and internally tagged enums are intentionally absent.
        let raw = <&'de RawValue>::deserialize(deserializer)?;
        let token = raw.get();
        if token.len() > MAX_NUMBER_BYTES {
            return Err(de::Error::custom("numeric token exceeds 128 bytes"));
        }
        // RawValue has already checked JSON grammar. Rust f64 parses the original
        // token once, preserving halfway rounding, subnormals and signed zero.
        let value = token.parse::<f64>().map_err(de::Error::custom)?;
        if !value.is_finite() {
            return Err(de::Error::custom("profile numbers must be finite"));
        }
        let significand = token.split(['e', 'E']).next().unwrap_or(token);
        if value == 0.0
            && significand
                .bytes()
                .any(|byte| (b'1'..=b'9').contains(&byte))
        {
            return Err(de::Error::custom(
                "nonzero decimal underflows binary64 to zero",
            ));
        }
        Ok(Self(value))
    }
}

/// A decoder bound, not merely a post-allocation length check. No size hint is
/// trusted, and the first excess element is rejected before deserializing it.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub(super) struct BoundedVec<T, const MAX: usize>(pub Vec<T>);

impl<'de, T: Deserialize<'de>, const MAX: usize> Deserialize<'de> for BoundedVec<T, MAX> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BoundedVisitor<T, const MAX: usize>(PhantomData<T>);
        struct Excess;
        impl<'de> Deserialize<'de> for Excess {
            fn deserialize<D: Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
                Err(de::Error::custom("profile array exceeds its element limit"))
            }
        }
        impl<'de, T: Deserialize<'de>, const MAX: usize> de::Visitor<'de> for BoundedVisitor<T, MAX> {
            type Value = BoundedVec<T, MAX>;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "an array with at most {MAX} elements")
            }
            fn visit_seq<A: de::SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::with_capacity(MAX.min(8));
                while values.len() < MAX {
                    match sequence.next_element()? {
                        Some(value) => values.push(value),
                        None => return Ok(BoundedVec(values)),
                    }
                }
                // next_element notices ']' without invoking Excess. Otherwise it
                // errors without parsing or allocating the excess element.
                let _ = sequence.next_element::<Excess>()?;
                Ok(BoundedVec(values))
            }
        }
        deserializer.deserialize_seq(BoundedVisitor::<T, MAX>(PhantomData))
    }
}

/// Supplement serde_json's recursion guard for RawValue's iterative subtree
/// scanner. This performs no allocations; grammar remains the JSON parser's job.
pub(super) fn check_depth(json: &str) -> Result<(), super::ProfileError> {
    let mut depth = 0_usize;
    let mut quoted = false;
    let mut escaped = false;
    for byte in json.bytes() {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
        } else {
            match byte {
                b'"' => quoted = true,
                b'[' | b'{' => {
                    depth += 1;
                    if depth > super::MAX_JSON_DEPTH {
                        return Err(super::ProfileError(
                            "profile JSON nesting exceeds 128 containers".into(),
                        ));
                    }
                }
                b']' | b'}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    Ok(())
}
