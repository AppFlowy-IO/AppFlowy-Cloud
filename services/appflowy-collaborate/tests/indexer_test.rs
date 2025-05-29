use collab::core::collab::{default_client_id, CollabOptions};
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_document::document::Document;
use workspace_template::document::getting_started::{
  get_initial_document_data, getting_started_document_data,
};

#[test]
fn document_plain_text() {
  let doc = getting_started_document_data().unwrap();
  let options = CollabOptions::new("1".to_string(), default_client_id());
  let collab = Collab::new_with_options(CollabOrigin::Server, options).unwrap();
  let document = Document::create_with_data(collab, doc).unwrap();
  let text = document.paragraphs().join("");
  let expected = "Welcome to AppFlowy$Download for macOS, Windows, and Linuxlink$$$quick startAsk AI powered by advanced AI models: chat, search, write, and much more ✨❤\u{fe0f}Love AppFlowy and open source? Follow our latest product updates:Twitter: @appflowyReddit: r/appflowyGithub";
  assert_eq!(&text, expected);
}

#[test]
fn document_plain_text_with_nested_blocks() {
  let doc = get_initial_document_data().unwrap();
  let options = CollabOptions::new("1".to_string(), default_client_id());
  let collab = Collab::new_with_options(CollabOrigin::Server, options).unwrap();
  let document = Document::create_with_data(collab, doc).unwrap();
  let text = document.paragraphs().join("");
  let expected = "Welcome to AppFlowy!Here are the basicsHere is H3Click anywhere and just start typing.Click Enter to create a new line.Highlight any text, and use the editing menu to styleyourwritinghowever you like.As soon as you type / a menu will pop up. Select different types of content blocks you can add.Type / followed by /bullet or /num to create a list.Click + New Page button at the bottom of your sidebar to add a new page.Click + next to any page title in the sidebar to quickly add a new subpage, Document, Grid, or Kanban Board.Keyboard shortcuts, markdown, and code blockKeyboard shortcuts guideMarkdown referenceType /code to insert a code block// This is the main function.\nfn main() {\n    // Print text to the console.\n    println!(\"Hello World!\");\n}This is a paragraphThis is a paragraphHave a question❓Click ? at the bottom right for help and support.This is a paragraphThis is a paragraphClick ? at the bottom right for help and support. Like AppFlowy? Follow us: GitHubTwitter: @appflowy Newsletter";
  assert_eq!(&text, expected);
}
