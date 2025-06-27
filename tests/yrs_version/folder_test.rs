use collab::core::collab::default_client_id;
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab_folder::Folder;

use crate::yrs_version::util::read_bytes_from_file;

/// Load collaboration data that was encoded using Yjs version 0.17.
/// folder structure:
///   favorite: document3
///   person-document2
///   person-document1
///     - person-document1-1
///     - person-document1-2
///   Getting started
///     - document1
///     - document2
///     - document3
///
#[test]
fn load_yrs_0172_version_folder_using_current_yrs_version() {
  let data = read_bytes_from_file("folder_encode_collab_0172");
  let encode_collab = EncodedCollab::decode_from_bytes(&data).unwrap();
  let uid = 322319512080748544;

  let folder = Folder::from_collab_doc_state(
    CollabOrigin::Empty,
    encode_collab.into(),
    "fake_id", // just use fake id
    default_client_id(),
  )
  .unwrap();

  let workspace_id = folder.get_workspace_id().unwrap();
  let views = folder.get_views_belong_to(&workspace_id, uid);
  assert_eq!(views.len(), 3);
  assert_eq!(views[0].name, "person-document2");
  assert_eq!(views[1].name, "person-document1");
  assert_eq!(views[2].name, "Getting started");

  let view_1_sub_views = folder.get_views_belong_to(&views[1].id, uid);
  assert_eq!(view_1_sub_views.len(), 2);
  assert_eq!(view_1_sub_views[0].name, "person-document1-1");
  assert_eq!(view_1_sub_views[1].name, "person-document1-2");

  let view_2_sub_views = folder.get_views_belong_to(&views[2].id, uid);
  assert_eq!(view_2_sub_views.len(), 3);
  assert_eq!(view_2_sub_views[0].name, "document1");
  assert_eq!(view_2_sub_views[1].name, "document2");
  assert_eq!(view_2_sub_views[2].name, "document3");

  let favorite_section_items = folder.get_my_favorite_sections(uid);
  assert_eq!(favorite_section_items.len(), 1);
  assert_eq!(view_2_sub_views[2].id, favorite_section_items[0].id);
}
