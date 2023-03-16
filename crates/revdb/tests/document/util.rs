use revdb::db::RevDB;
use tempfile::TempDir;

pub fn make_test_db() -> RevDB {
    let tempdir = TempDir::new().unwrap();
    RevDB::open(tempdir).unwrap()
}
