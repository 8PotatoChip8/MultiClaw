use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use anyhow::{anyhow, Result};
use rand::RngCore;
use std::fs;

pub struct CryptoMaster {
    cipher: Aes256Gcm,
}

impl CryptoMaster {
    pub fn new(key_path: &str) -> Result<Self> {
        let key_hex = fs::read_to_string(key_path)?;
        let key_bytes = hex::decode(key_hex.trim())?;
        if key_bytes.len() != 32 {
            return Err(anyhow!("Master key must be exactly 32 bytes"));
        }
        let cipher = Aes256Gcm::new_from_slice(&key_bytes).map_err(|e| anyhow!("{e}"))?;
        Ok(Self { cipher })
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = self.cipher.encrypt(nonce, plaintext).map_err(|e| anyhow!("{e}"))?;
        
        // prepend nonce to ciphertext for storage
        let mut out = nonce_bytes.to_vec();
        out.extend(ciphertext);
        Ok(out)
    }

    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        if data.len() < 12 {
            return Err(anyhow!("Invalid ciphertext length"));
        }
        let nonce = Nonce::from_slice(&data[0..12]);
        let plaintext = self.cipher.decrypt(nonce, &data[12..]).map_err(|e| anyhow!("{e}"))?;
        Ok(plaintext)
    }
}
