use serde::{de::Error, Deserialize, Deserializer};
use solana_sdk::signature::Keypair;

pub fn deserialize_keypair<'de, D>(deserializer: D) -> Result<Keypair, D::Error>
where D: Deserializer<'de> {
    let s = String::deserialize(deserializer).map_err(D::Error::custom)?;
    Keypair::try_from_base58_string(&s).map_err(D::Error::custom)
}

pub(super) fn max_of_option<T: Ord>(left: Option<T>, right: Option<T>) -> Option<T> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}
