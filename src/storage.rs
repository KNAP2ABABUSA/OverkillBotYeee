use std::collections::{HashSet, HashMap};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;



const BLOCK_FILE: &str = "blocks.bin";


#[derive(Serialize, Deserialize, Clone, Debug)]
struct BlockStore {
    anons: HashSet<String>,
    usr: HashSet<String>,
    ids: HashSet<i64>,
    a2u: HashMap<String, i64>,
}

impl Default for BlockStore {
    fn default() -> Self {
        Self {
            anons: HashSet::new(),
            usr: HashSet::new(),
            ids: HashSet::new(),
            a2u: HashMap::new()
        }
    }
}

#[derive(Clone)]
pub struct BlockMgr {
    s: Arc<RwLock<BlockStore>>,
}

impl BlockMgr {
    pub fn new() -> Self {
        Self { s: Arc::new(RwLock::new(Self::load().unwrap_or_default())) }
    }

    fn load() -> Result<BlockStore, Box<dyn std::error::Error>> {
        if !Path::new(BLOCK_FILE).exists() {
            return Ok(BlockStore::default());
        }
        let mut f = File::open(BLOCK_FILE)?;
        let mut b = Vec::new();
        f.read_to_end(&mut b)?;
        let d: Vec<u8> = b.iter().map(|b| b ^ 0xBB).collect();
        Ok(bincode::deserialize(&d)?)
    }

    async fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let s = self.s.read().await;
        let e = bincode::serialize(&*s)?;
        let enc: Vec<u8> = e.iter().map(|b| b ^ 0xBB).collect();
        let tmp = format!("{}.tmp", BLOCK_FILE);
        let mut f = File::create(&tmp)?;
        f.write_all(&enc)?;
        f.sync_all()?;
        fs::rename(tmp, BLOCK_FILE)?;
        Ok(())
    }

    pub async fn block_anon(&self, c: &str) -> bool {
        let k = format!("anon{}", c);
        let mut s = self.s.write().await;
        let ins = s.anons.insert(k);
        if ins { drop(s); self.save().await.ok(); }
        ins
    }

    pub async fn unblock_anon(&self, c: &str) -> bool {
        let k = format!("anon{}", c);
        let mut s = self.s.write().await;
        let rem = s.anons.remove(&k);
        if rem { drop(s); self.save().await.ok(); }
        rem
    }

    pub async fn block_user(&self, u: &str) -> bool {
        let mut s = self.s.write().await;
        let ins = s.usr.insert(u.into());
        if ins { drop(s); self.save().await.ok(); }
        ins
    }

    pub async fn unblock_user(&self, u: &str) -> bool {
        let mut s = self.s.write().await;
        let rem = s.usr.remove(u);
        if rem { drop(s); self.save().await.ok(); }
        rem
    }

    pub async fn block_id(&self, uid: i64) -> bool {
        let mut s = self.s.write().await;
        let ins = s.ids.insert(uid);
        if ins { drop(s); self.save().await.ok(); }
        ins
    }

    pub async fn reg_anon(&self, c: &str, uid: i64) {
        let mut s = self.s.write().await;
        s.a2u.insert(format!("anon{}", c), uid);
        drop(s); self.save().await.ok();
    }

    pub async fn get_by_anon(&self, c: &str) -> Option<i64> {
        self.s.read().await.a2u.get(&format!("anon{}", c)).copied()
    }


    #[allow(dead_code)]
    pub async fn unblock_all(&self) -> (usize, usize, usize) {
        let mut s = self.s.write().await;
        let i = s.ids.len();
        let u = s.usr.len();
        let a = s.anons.len();
        s.ids.clear();
        s.usr.clear();
        s.anons.clear();
        drop(s);
        self.save().await.ok();
        (i, u, a)
    }

    pub async fn is_anon_blocked(&self, c: &str) -> bool {
        self.s.read().await.anons.contains(&format!("anon{}", c))
    }

    pub async fn is_user_blocked(&self, u: &str) -> bool {
        self.s.read().await.usr.contains(u)
    }

    pub async fn is_id_blocked(&self, uid: i64) -> bool {
        self.s.read().await.ids.contains(&uid)
    }

    #[allow(dead_code)]
    pub async fn blocked_list(&self) -> Vec<String> {
        let s = self.s.read().await;
        let mut r = Vec::new();
        for a in &s.anons {
            r.push(format!("{} (пользователь: {:?})", a, s.a2u.get(a)));
        }
        for u in &s.usr {
            r.push(u.clone());
        }
        for &i in &s.ids {
            r.push(format!("ID:{}", i));
        }
        r
    }
}
//Хранилище для моей бяки