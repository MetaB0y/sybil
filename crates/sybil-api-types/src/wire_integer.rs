//! Decimal-string JSON encoding for protocol-sized integer fields.
//!
//! JavaScript cannot represent every Rust `u64` or `i64` as a JSON number.
//! API DTOs therefore keep integers internally while using these serde
//! adapters to emit exact decimal strings. Deserialization also accepts legacy
//! integer tokens so non-JavaScript clients can migrate without a flag day.

use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serializer};

pub fn serialize<T, S>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    T: fmt::Display,
    S: Serializer,
{
    serializer.serialize_str(&value.to_string())
}

pub fn deserialize<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: FromStr + TryFrom<i64> + TryFrom<u64>,
    T::Err: fmt::Display,
    <T as TryFrom<i64>>::Error: fmt::Display,
    <T as TryFrom<u64>>::Error: fmt::Display,
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(DecimalIntegerVisitor(PhantomData))
}

struct DecimalIntegerVisitor<T>(PhantomData<T>);

impl<'de, T> Visitor<'de> for DecimalIntegerVisitor<T>
where
    T: FromStr + TryFrom<i64> + TryFrom<u64>,
    T::Err: fmt::Display,
    <T as TryFrom<i64>>::Error: fmt::Display,
    <T as TryFrom<u64>>::Error: fmt::Display,
{
    type Value = T;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a base-10 integer string")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        value.parse().map_err(E::custom)
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_str(&value)
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        T::try_from(value).map_err(E::custom)
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        T::try_from(value).map_err(E::custom)
    }
}

pub mod option {
    use super::*;

    pub fn serialize<T, S>(value: &Option<T>, serializer: S) -> Result<S::Ok, S::Error>
    where
        T: fmt::Display,
        S: Serializer,
    {
        match value {
            Some(value) => serializer.serialize_some(&value.to_string()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
    where
        T: FromStr + TryFrom<i64> + TryFrom<u64>,
        T::Err: fmt::Display,
        <T as TryFrom<i64>>::Error: fmt::Display,
        <T as TryFrom<u64>>::Error: fmt::Display,
        D: Deserializer<'de>,
    {
        deserializer.deserialize_option(DecimalOptionVisitor(PhantomData))
    }

    struct DecimalOptionVisitor<T>(PhantomData<T>);

    impl<'de, T> Visitor<'de> for DecimalOptionVisitor<T>
    where
        T: FromStr + TryFrom<i64> + TryFrom<u64>,
        T::Err: fmt::Display,
        <T as TryFrom<i64>>::Error: fmt::Display,
        <T as TryFrom<u64>>::Error: fmt::Display,
    {
        type Value = Option<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("null or a base-10 integer string")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            super::deserialize(deserializer).map(Some)
        }
    }
}

pub mod map_vec_u64 {
    use super::*;

    pub fn serialize<S>(value: &HashMap<String, Vec<u64>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(value.len()))?;
        for (key, values) in value {
            map.serialize_entry(key, &DecimalU64Slice(values))?;
        }
        map.end()
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<HashMap<String, Vec<u64>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = HashMap::<String, Vec<DecimalU64>>::deserialize(deserializer)?;
        Ok(values
            .into_iter()
            .map(|(key, values)| (key, values.into_iter().map(|value| value.0).collect()))
            .collect())
    }

    struct DecimalU64(u64);

    impl<'de> Deserialize<'de> for DecimalU64 {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            super::deserialize(deserializer).map(Self)
        }
    }

    struct DecimalU64Slice<'a>(&'a [u64]);

    impl serde::Serialize for DecimalU64Slice<'_> {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
            for value in self.0 {
                sequence.serialize_element(&value.to_string())?;
            }
            sequence.end()
        }
    }
}

pub mod map_u32_u64 {
    use super::*;

    pub fn serialize<S>(value: &HashMap<u32, u64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(value.len()))?;
        for (key, value) in value {
            map.serialize_entry(key, &value.to_string())?;
        }
        map.end()
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<HashMap<u32, u64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = HashMap::<u32, DecimalU64>::deserialize(deserializer)?;
        Ok(values
            .into_iter()
            .map(|(key, value)| (key, value.0))
            .collect())
    }

    struct DecimalU64(u64);

    impl<'de> Deserialize<'de> for DecimalU64 {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            super::deserialize(deserializer).map(Self)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct WireValues {
        #[serde(with = "super")]
        unsigned_nanos: u64,
        #[serde(with = "super")]
        signed_nanos: i64,
        #[serde(with = "super::option")]
        optional_nanos: Option<u64>,
        #[serde(with = "super::map_vec_u64")]
        prices_nanos: HashMap<String, Vec<u64>>,
        #[serde(with = "super::map_u32_u64")]
        reference_prices_nanos: HashMap<u32, u64>,
    }

    #[test]
    fn preserves_integers_above_javascript_safe_range() {
        let value = WireValues {
            unsigned_nanos: u64::MAX,
            signed_nanos: i64::MIN,
            optional_nanos: Some(9_007_199_254_740_993),
            prices_nanos: HashMap::from([(
                "market".to_string(),
                vec![9_007_199_254_740_993, u64::MAX],
            )]),
            reference_prices_nanos: HashMap::from([(7, u64::MAX)]),
        };

        let json = serde_json::to_value(&value).expect("serialize exact integers");
        assert_eq!(json["unsigned_nanos"], u64::MAX.to_string());
        assert_eq!(json["signed_nanos"], i64::MIN.to_string());
        assert_eq!(json["optional_nanos"], "9007199254740993");
        assert_eq!(
            json["prices_nanos"]["market"],
            json!(["9007199254740993", "18446744073709551615"])
        );
        assert_eq!(json["reference_prices_nanos"]["7"], "18446744073709551615");
        assert_eq!(
            serde_json::from_value::<WireValues>(json).expect("round trip"),
            value
        );
    }

    #[test]
    fn accepts_legacy_integer_tokens_without_accepting_floats() {
        let legacy = json!({
            "unsigned_nanos": 42,
            "signed_nanos": -7,
            "optional_nanos": null,
            "prices_nanos": {"market": [1, 2]},
            "reference_prices_nanos": {"7": 3}
        });
        let value = serde_json::from_value::<WireValues>(legacy).expect("legacy integers");
        assert_eq!(value.unsigned_nanos, 42);
        assert_eq!(value.signed_nanos, -7);
        assert_eq!(value.optional_nanos, None);
        assert_eq!(value.prices_nanos["market"], [1, 2]);
        assert_eq!(value.reference_prices_nanos[&7], 3);

        let floating = r#"{
            "unsigned_nanos": 1.5,
            "signed_nanos": -7,
            "optional_nanos": null,
            "prices_nanos": {},
            "reference_prices_nanos": {}
        }"#;
        assert!(serde_json::from_str::<WireValues>(floating).is_err());
    }
}
