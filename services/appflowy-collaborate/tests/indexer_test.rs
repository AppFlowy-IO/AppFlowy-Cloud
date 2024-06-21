use appflowy_collaborate::indexer::DocumentDataExt;
use workspace_template::document::get_started::{
  get_initial_document_data, get_started_document_data,
};

#[test]
fn document_plain_text() {
  let doc = get_started_document_data().unwrap();
  let text = doc.to_plain_text();
  let expected = "Welcome to AppFlowy! Here are the basics Click anywhere and just start typing. Highlight any text, and use the editing menu to style your writing however you like. As soon as you type / a menu will pop up. Select different types of content blocks you can add. Type / followed by /bullet or /num to create a list. Click + New Page button at the bottom of your sidebar to add a new page. Click + next to any page title in the sidebar to quickly add a new subpage, Document , Grid , or Kanban Board . Keyboard shortcuts, markdown, and code block Keyboard shortcuts guide Markdown reference Type /code to insert a code block // This is the main function.\nfn main() {\n    // Print text to the console.\n    println!(\"Hello World!\");\n} Have a question❓ Click ? at the bottom right for help and support. Like AppFlowy? Follow us: GitHub Twitter : @appflowy Newsletter ";
  assert_eq!(&text, expected);
}

#[test]
fn document_plain_text_with_nested_blocks() {
  let doc = get_initial_document_data().unwrap();
  let text = doc.to_plain_text();
  let expected = "Welcome to AppFlowy! Here are the basics Here is H3 Click anywhere and just start typing. Click Enter to create a new line. Highlight any text, and use the editing menu to style your writing however you like. As soon as you type / a menu will pop up. Select different types of content blocks you can add. Type / followed by /bullet or /num to create a list. Click + New Page button at the bottom of your sidebar to add a new page. Click + next to any page title in the sidebar to quickly add a new subpage, Document , Grid , or Kanban Board . Keyboard shortcuts, markdown, and code block Keyboard shortcuts guide Markdown reference Type /code to insert a code block // This is the main function.\nfn main() {\n    // Print text to the console.\n    println!(\"Hello World!\");\n} This is a paragraph This is a paragraph Have a question❓ Click ? at the bottom right for help and support. This is a paragraph This is a paragraph Click ? at the bottom right for help and support. Like AppFlowy? Follow us: GitHub Twitter : @appflowy Newsletter ";
  assert_eq!(&text, expected);
}
