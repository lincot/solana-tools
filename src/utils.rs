use serde::{de::Error, Deserialize, Deserializer};
use solana_sdk::{bs58, signature::Keypair};

pub fn deserialize_keypair<'de, D>(deserializer: D) -> Result<Keypair, D::Error>
where D: Deserializer<'de> {
    let s = String::deserialize(deserializer).map_err(D::Error::custom)?;
    let keydata = bs58::decode(s).into_vec().map_err(D::Error::custom)?;
    Keypair::from_bytes(&keydata).map_err(D::Error::custom)
}
