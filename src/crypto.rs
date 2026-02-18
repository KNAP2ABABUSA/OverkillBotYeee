use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use sha2::{Digest, Sha256};
use std::fs;
use zeroize::Zeroize;
const KEY_FILE: &str = ".enc_key";
pub struct SecureCrypto {
    cipher: Aes256Gcm,
}
impl SecureCrypto {
    pub fn new() -> Self {
        let key = Self::load_or_create_key();
        let cipher = Aes256Gcm::new(&key.into());
        Self { cipher }
    }
    fn load_or_create_key() -> [u8; 32] {
        if let Ok(data) = fs::read(KEY_FILE) {
            if data.len() == 32 {
                let mut key = [0u8; 32];
                key.copy_from_slice(&data);
                return key;
            }
        }
        let mut key = [0u8; 32];
        use rand::RngCore;
        rand::thread_rng().fill_bytes(&mut key);
        let _ = fs::write(KEY_FILE, &key);
        key
    }
    pub fn encrypt(&self, data: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut nonce_bytes = [0u8; 12];
        use rand::RngCore;
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = self.cipher.encrypt(nonce, data)?;
        let mut result = Vec::with_capacity(12 + ciphertext.len());
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&ciphertext);
        Ok(result)
    }
    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        if data.len() < 12 {
            return Err("Invalid data".into());
        }
        let nonce = Nonce::from_slice(&data[..12]);
        let ciphertext = &data[12..];
        let plaintext = self.cipher.decrypt(nonce, ciphertext)?;
        Ok(plaintext)
    }
}
pub fn hash_id(id: i64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(id.to_le_bytes());
    hasher.update(b"OVERKILL_SALT_2025");
    let result = hasher.finalize();
    hex::encode(&result[..16])
}
pub fn hash_username(username: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(username.as_bytes());
    hasher.update(b"OVERKILL_USERNAME_SALT");
    let result = hasher.finalize();
    hex::encode(&result[..16])
}
//Типо шифрашка по AES256