use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use rand::Rng;
const MUSOR: &str = "anon_keys.bin";
#[derive(Serialize, Deserialize, Clone, Debug)]
struct AnonymousStorage {
    user_id_to_code: HashMap<i64, u16>,
    code_to_user_id: HashMap<u16, i64>,
    used_codes: Vec<u16>,
}
impl Default for AnonymousStorage {
    fn default() -> Self {
        Self {
            user_id_to_code: HashMap::new(),
            code_to_user_id: HashMap::new(),
            used_codes: Vec::new(),
        }
    }
}
fn generate_unique_code(existing_codes: &[u16]) -> u16 {
    let mut random_generator = rand::thread_rng();
    loop {
        let code = random_generator.gen_range(1000..=9999);
        if !existing_codes.contains(&code) {
            return code;
        }
    }
}
#[derive(Clone)]
pub struct AnonymousManager {
    storage: Arc<Mutex<AnonymousStorage>>,
}
impl AnonymousManager {
    pub fn new() -> Self {
        let storage = Self::load_storage().unwrap_or_default();
        Self { storage: Arc::new(Mutex::new(storage)) }
    }
    fn load_storage() -> Result<AnonymousStorage, Box<dyn std::error::Error>> {
        if !Path::new(MUSOR).exists() {
            return Ok(AnonymousStorage::default());
        }
        let mut file = File::open(MUSOR)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;
        let decrypted: Vec<u8> = buffer.iter().map(|byte| byte ^ 0xAA).collect();
        Ok(bincode::deserialize(&decrypted)?)
    }
    async fn save_storage(&self) -> Result<(), Box<dyn std::error::Error>> {
        let storage = self.storage.lock().await;
        let encoded = bincode::serialize(&*storage)?;
        let encrypted: Vec<u8> = encoded.iter().map(|byte| byte ^ 0xAA).collect();
        let temporary_file = format!("{}.tmp", MUSOR);
        let mut file = File::create(&temporary_file)?;
        file.write_all(&encrypted)?;
        file.sync_all()?;
        fs::rename(temporary_file, MUSOR)?;
        Ok(())
    }
    pub async fn get_or_create_anonymous_code(&self, user_id: i64) -> u16 {
        {
            let storage = self.storage.lock().await;
            if let Some(&code) = storage.user_id_to_code.get(&user_id) {
                return code;
            }
        }
        let new_code = {
            let storage = self.storage.lock().await;
            generate_unique_code(&storage.used_codes)
        };
        {
            let mut storage = self.storage.lock().await;
            storage.user_id_to_code.insert(user_id, new_code);
            storage.code_to_user_id.insert(new_code, user_id);
            storage.used_codes.push(new_code);
        }
        self.save_storage().await.ok();
        new_code
    }
    pub async fn get_anonymous_code(&self, user_id: i64) -> Option<u16> {
        let storage = self.storage.lock().await;
        storage.user_id_to_code.get(&user_id).copied()
    }
}
//Фигня для имен анонимов