use crate::db::RevDB;
use crate::error::RevDBError;
use crate::range::RevRange;
use serde::{Deserialize, Serialize};

pub struct Document<'a> {
    pub(crate) db: &'a RevDB,
}

impl<'a> Document<'a> {
    pub fn insert(
        &self,
        uid: i64,
        document_id: i64,
        rev_id: i64,
        data: &[u8],
    ) -> Result<(), RevDBError> {
        let key = make_document_key(uid, document_id, rev_id);
        let _ = self.db.insert(key, data)?;
        Ok(())
    }

    pub fn get(
        &self,
        uid: i64,
        document_id: i64,
        rev_id: i64,
    ) -> Result<Option<Vec<u8>>, RevDBError> {
        let key = make_document_key(uid, document_id, rev_id);
        let value = self.db.get(key)?;
        Ok(value.map(|value| value.to_vec()))
    }

    pub fn get_with_range(
        &self,
        uid: i64,
        document_id: i64,
        range: RevRange,
    ) -> Result<Vec<DocumentRevData>, RevDBError> {
        let from = make_document_key(uid, document_id, range.start);
        let to = make_document_key(uid, document_id, range.end);
        self.batch_get(from, to)
    }

    pub fn get_after(
        &self,
        uid: i64,
        document_id: i64,
        rev_id: i64,
    ) -> Result<Vec<DocumentRevData>, RevDBError> {
        let from = make_document_key(uid, document_id, rev_id);
        let to = make_document_key(uid, document_id, i64::MAX);
        self.batch_get(from, to)
    }

    pub fn get_before(
        &self,
        uid: i64,
        document_id: i64,
        rev_id: i64,
    ) -> Result<Vec<DocumentRevData>, RevDBError> {
        let from = make_document_key(uid, document_id, 0);
        let to = make_document_key(uid, document_id, rev_id);
        self.batch_get(from, to)
    }

    pub fn batch_get<K: AsRef<[u8]>>(
        &self,
        from: K,
        to: K,
    ) -> Result<Vec<DocumentRevData>, RevDBError> {
        let items = self.db.batch_get(from, to)?;
        let mut document_revs = vec![];
        for item in items {
            let rev_data = DocumentRevData::from_vec(item.as_ref())?;
            document_revs.push(rev_data);
        }
        Ok(document_revs)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentRevData {
    #[serde(rename = "rid")]
    pub rev_id: i64,

    #[serde(rename = "bid")]
    pub base_rev_id: i64,

    #[serde(rename = "data")]
    pub content: String,
}

impl DocumentRevData {
    pub fn from_vec(data: &[u8]) -> Result<Self, RevDBError> {
        bincode::deserialize::<Self>(data).map_err(|_e| RevDBError::SerdeError)
    }

    pub fn to_vec(&self) -> Result<Vec<u8>, RevDBError> {
        bincode::serialize(self).map_err(|_e| RevDBError::SerdeError)
    }
}

// Optimize your data layout: Sled's B-Tree implementation works best when the keys are sequential,
// so try to organize the data in a way that maximizes sequential access.
fn make_document_key(uid: i64, document_id: i64, rev_id: i64) -> [u8; 24] {
    let mut key = [0; 24];
    key[0..8].copy_from_slice(&uid.to_be_bytes());
    key[8..16].copy_from_slice(&document_id.to_be_bytes());
    key[16..24].copy_from_slice(&rev_id.to_be_bytes());
    key
}
