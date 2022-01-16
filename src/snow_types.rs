use std::hash::Hash;

use serde::{Serialize, Deserialize};
use snow::{Builder, Keypair};

static NOISE_PARAMS: &str = "Noise_IX_25519_ChaChaPoly_BLAKE2s";
pub fn builder<'a>() -> Builder<'a> {
    snow::Builder::new(NOISE_PARAMS.parse().unwrap())
}

pub struct SnowKeypair(Keypair);
#[derive(Clone, Debug, Serialize, Deserialize, PartialOrd, Eq, PartialEq, Hash)]
pub struct SnowPublicKey(Vec<u8>);
#[derive(Clone, Debug)]
pub struct SnowPrivateKey(Vec<u8>);

impl SnowKeypair {
    pub fn new() -> SnowKeypair {
        let keypair = builder().generate_keypair().unwrap();
        SnowKeypair(keypair)
    }
    pub fn public(&self) -> SnowPublicKey {
        SnowPublicKey(self.0.public.clone())
    }
    pub fn private(&self) -> SnowPrivateKey {
        SnowPrivateKey(self.0.private.clone())
    }
    pub fn builder(&self) -> Builder<'_> {
        builder().local_private_key(&self.0.private)
    }
}