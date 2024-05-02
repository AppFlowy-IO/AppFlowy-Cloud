use crate::yrs_version::util::read_bytes_from_file;
use collab::core::collab::DataSource;
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab_document::document::Document;

/// Load collaboration data that was encoded using Yjs version 0.17.
#[test]
fn load_yrs_0172_version_get_started_document_using_current_yrs_version() {
  let data = read_bytes_from_file("get_started_encode_collab_0172");
  let encode_collab = EncodedCollab::decode_from_bytes(&data).unwrap();

  let document = Document::from_doc_state(
    CollabOrigin::Empty,
    DataSource::DocStateV1(encode_collab.doc_state.to_vec()),
    "fake_id",
    vec![],
  )
  .unwrap();

  let document_data = document.get_document_data().unwrap();
  assert_eq!(document_data.blocks.len(), 25);
  let first_block = document_data.blocks.get(&document_data.page_id).unwrap();
  assert_eq!(first_block.id, document_data.page_id);

  let icon_block = document_data.blocks.get("a9SSKQKF4-").unwrap();
  let icon_data = icon_block.data.get("icon").unwrap().as_str().unwrap();
  assert_eq!(icon_data, "🥰");

  let welcome_to_appflowy = document_data
    .meta
    .text_map
    .as_ref()
    .unwrap()
    .get("OETXfTYZEw")
    .unwrap();
  assert_eq!(
    welcome_to_appflowy,
    r#"[{"insert":"Welcome to AppFlowy!"}]"#
  );
}
