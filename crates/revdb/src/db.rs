use crate::error::RevDBError;
use sled::Db;
use std::path::Path;

pub struct RevDB {
    pub(crate) db: Db,
}

impl RevDB {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RevDBError> {
        let db = sled::open(path)?;
        Ok(Self { db })
    }

    pub fn insert(&self, uid: i64, rev_id: i64, data: &[u8]) -> Result<(), RevDBError> {
        let key = make_seq_key(uid, rev_id);
        let _ = self.db.insert(key, data)?;
        Ok(())
    }

    pub fn get(&self, uid: i64, rev_id: i64) -> Result<Option<Vec<u8>>, RevDBError> {
        let key = make_seq_key(uid, rev_id);
        let value = self.db.get(key)?;
        Ok(value.map(|value| value.to_vec()))
    }
}

// Optimize your data layout: Sled's B-Tree implementation works best when the keys are sequential,
// so try to organize the data in a way that maximizes sequential access.
fn make_seq_key(uid: i64, rev_id: i64) -> [u8; 16] {
    let mut key = [0; 16];
    key[0..8].copy_from_slice(&uid.to_be_bytes());
    key[8..16].copy_from_slice(&rev_id.to_be_bytes());
    key
}
