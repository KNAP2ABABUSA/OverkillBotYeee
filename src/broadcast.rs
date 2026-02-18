use std::collections::{HashSet, HashMap};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
const BROADCAST_STORAGE_FILE: &str = "bc_list.bin";
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BroadcastStorage {
    all_users: HashSet<i64>,
    unsubscribed_users: HashSet<i64>,
    username_to_user_id: HashMap<String, i64>,
}
impl Default for BroadcastStorage {
    fn default() -> Self {
        Self {
            all_users: HashSet::new(),
            unsubscribed_users: HashSet::new(),
            username_to_user_id: HashMap::new()
        }
    }
}
#[derive(Clone)]
pub struct BroadcastManager {
    storage: Arc<RwLock<BroadcastStorage>>,
}
impl BroadcastManager {
    pub fn new() -> Self {
        let storage = Self::load_storage().unwrap_or_default();
        Self { storage: Arc::new(RwLock::new(storage)) }
    }
    fn load_storage() -> Result<BroadcastStorage, Box<dyn std::error::Error>> {
        if !Path::new(BROADCAST_STORAGE_FILE).exists() {
            return Ok(BroadcastStorage::default());
        }
        let mut file = File::open(BROADCAST_STORAGE_FILE)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;
        let decrypted: Vec<u8> = buffer.iter().map(|byte| byte ^ 0xCC).collect();
        Ok(bincode::deserialize(&decrypted)?)
    }
    async fn save_storage(&self) -> Result<(), Box<dyn std::error::Error>> {
        let storage = self.storage.read().await;
        let encoded = bincode::serialize(&*storage)?;
        let encrypted: Vec<u8> = encoded.iter().map(|byte| byte ^ 0xCC).collect();
        let temporary_file = format!("{}.tmp", BROADCAST_STORAGE_FILE);
        let mut file = File::create(&temporary_file)?;
        file.write_all(&encrypted)?;
        file.sync_all()?;
        fs::rename(temporary_file, BROADCAST_STORAGE_FILE)?;
        Ok(())
    }
    pub async fn add_user_with_username(&self, user_id: i64, username: Option<String>) {
        let mut storage = self.storage.write().await;
        let mut changed = storage.all_users.insert(user_id);
        if let Some(username) = username {
            storage.username_to_user_id.insert(username, user_id);
            changed = true;
        }
        if changed {
            drop(storage);
            self.save_storage().await.ok();
        }
    }
    pub async fn get_broadcast_list(&self) -> Vec<i64> {
        let storage = self.storage.read().await;
        storage.all_users.iter()
            .filter(|user_id| !storage.unsubscribed_users.contains(*user_id))
            .copied()
            .collect()
    }
    pub async fn get_user_by_username(&self, username: &str) -> Option<i64> {
        let storage = self.storage.read().await;
        storage.username_to_user_id.get(username).copied()
    }
}
//Это рассылка. Для удобства изменения я кинул ее в отдельный файл