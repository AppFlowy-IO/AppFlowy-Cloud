use crate::document::Document;
use crate::error::RevDBError;
use sled::{Batch, Db, IVec};
use std::path::Path;

pub struct RevDB {
    pub(crate) db: Db,
}

impl RevDB {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RevDBError> {
        let db = sled::open(path)?;
        Ok(Self { db })
    }

    pub fn document(&self) -> Document {
        Document { db: self }
    }

    pub fn get<K: AsRef<[u8]>>(&self, key: K) -> Result<Option<IVec>, RevDBError> {
        let value = self.db.get(key)?;
        Ok(value)
    }

    pub fn batch_get<K: AsRef<[u8]>>(
        &self,
        from_key: K,
        to_key: K,
    ) -> Result<Vec<IVec>, RevDBError> {
        let iter = self.db.range(from_key..=to_key);
        let mut items = vec![];
        for item in iter {
            let (_, value) = item?;
            items.push(value)
        }
        Ok(items)
    }

    pub fn insert<K: AsRef<[u8]>>(&self, key: K, value: &[u8]) -> Result<(), RevDBError> {
        let _ = self.db.insert(key, value)?;
        Ok(())
    }

    pub fn batch_insert<'a, K: AsRef<[u8]>>(
        &self,
        items: impl IntoIterator<Item = (K, &'a [u8])>,
    ) -> Result<(), RevDBError> {
        let mut batch = Batch::default();
        let items = items.into_iter();
        items.for_each(|(key, value)| {
            batch.insert(key.as_ref(), value);
        });
        let _ = self.db.apply_batch(batch)?;
        Ok(())
    }
}
