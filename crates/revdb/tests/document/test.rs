use crate::document::util::make_test_db;
use revdb::document::{Document, DocumentRevData};
use revdb::range::RevRange;

#[test]
fn insert_text() {
    let db = make_test_db();
    let document = db.document();
    let uid = 12345678;
    let document_id = 1;
    let value = DocumentRevData {
        rev_id: 0,
        base_rev_id: 0,
        content: "hello world".to_string(),
    };
    document.insert(uid, document_id, value.clone()).unwrap();

    let restored_data = document.get(uid, document_id, 0).unwrap().unwrap();
    assert_eq!(value.content, restored_data.content);
}

//noinspection RsExternalLinter
#[test]
fn insert_multi_text() {
    let db = make_test_db();
    let document = db.document();
    let uid = 12345678;
    let document_id = 1;

    let mut base_rev_id = 0;
    let mut expected_str = "".to_string();
    for i in 0..=100 {
        let content = i.to_string();
        expected_str.push_str(&content);
        let value = DocumentRevData {
            rev_id: i,
            base_rev_id,
            content,
        };
        base_rev_id += 1;
        document.insert(uid, document_id, value).unwrap();
    }
    //
    let restored_str = document
        .get_with_range(uid, document_id, RevRange::new(0, 100))
        .unwrap()
        .into_iter()
        .map(|data| data.content)
        .collect::<Vec<String>>()
        .join("");

    assert_eq!(expected_str, restored_str);
}

//noinspection RsExternalLinter
fn insert_100_string_to_document(uid: i64, document_id: i64, document: &Document) {
    let mut base_rev_id = 0;
    for i in 0..=100 {
        let content = i.to_string();
        let value = DocumentRevData {
            rev_id: i,
            base_rev_id,
            content,
        };
        base_rev_id += 1;
        document.insert(uid, document_id, value).unwrap();
    }
}

fn values_to_string(values: Vec<DocumentRevData>) -> String {
    values
        .into_iter()
        .map(|data| data.content)
        .collect::<Vec<String>>()
        .join("")
}

#[test]
fn get_value_before() {
    let db = make_test_db();
    let document = db.document();
    let uid = 12345678;
    let document_id = 1;
    insert_100_string_to_document(uid, document_id, &document);

    let restored_str = values_to_string(document.get_before(uid, document_id, 50).unwrap());
    assert_eq!("01234567891011121314151617181920212223242526272829303132333435363738394041424344454647484950", restored_str);

    let restored_str = values_to_string(document.get_before(uid, document_id, 0).unwrap());
    assert_eq!("0", restored_str);
}

#[test]
fn get_value_after() {
    let db = make_test_db();
    let document = db.document();
    let uid = 12345678;
    let document_id = 1;
    insert_100_string_to_document(uid, document_id, &document);

    let restored_str = values_to_string(document.get_after(uid, document_id, 50).unwrap());
    assert_eq!("5051525354555657585960616263646566676869707172737475767778798081828384858687888990919293949596979899100", restored_str);

    let restored_str = values_to_string(document.get_after(uid, document_id, 100).unwrap());
    assert_eq!("100", restored_str);
}

#[test]
fn get_value_with_range() {
    let db = make_test_db();
    let document = db.document();
    let uid = 12345678;
    let document_id = 1;
    insert_100_string_to_document(uid, document_id, &document);

    let restored_str = values_to_string(
        document
            .get_with_range(uid, document_id, RevRange::new(50, 60))
            .unwrap(),
    );
    assert_eq!("5051525354555657585960", restored_str);

    let restored_str = values_to_string(
        document
            .get_with_range(uid, document_id, RevRange::new(50, 200))
            .unwrap(),
    );
    assert_eq!("5051525354555657585960616263646566676869707172737475767778798081828384858687888990919293949596979899100", restored_str);

    let restored_str = values_to_string(
        document
            .get_with_range(uid, document_id, RevRange::new(50, 50))
            .unwrap(),
    );
    assert_eq!("50", restored_str);
}
