use collab_document::blocks::{DocumentData, TextDelta};

pub fn document_to_plain_text(document: &DocumentData) -> String {
  let mut buf = String::new();
  if let Some(text_map) = document.meta.text_map.as_ref() {
    let mut stack = Vec::new();
    stack.push(&document.page_id);
    // do a depth-first scan of the document blocks
    while let Some(block_id) = stack.pop() {
      if let Some(block) = document.blocks.get(block_id) {
        if let Some(deltas) = get_delta_from_block_data(block) {
          push_deltas_to_str(&mut buf, deltas);
        } else if let Some(deltas) = get_delta_from_external_text_id(block, text_map) {
          push_deltas_to_str(&mut buf, deltas);
        }
        if let Some(children) = document.meta.children_map.get(&block.children) {
          // we want to process children blocks in the same order they are given in children_map
          // however stack.pop gives us the last element first, so we push children
          // in reverse order
          stack.extend(children.iter().rev());
        }
      }
    }
  }
  //tracing::trace!("Document plain text: `{}`", buf);
  buf
}

/// Try to retrieve deltas from `block.data.delta`.
fn get_delta_from_block_data(block: &collab_document::blocks::Block) -> Option<Vec<TextDelta>> {
  if let Some(delta) = block.data.get("delta") {
    if let Ok(deltas) = serde_json::from_value::<Vec<TextDelta>>(delta.clone()) {
      return Some(deltas);
    }
  }
  None
}

/// Try to retrieve deltas from text_map's text associated with `block.external_id`.
fn get_delta_from_external_text_id(
  block: &collab_document::blocks::Block,
  text_map: &std::collections::HashMap<String, String>,
) -> Option<Vec<TextDelta>> {
  if block.external_type.as_deref() == Some("text") {
    if let Some(text_id) = block.external_id.as_deref() {
      if let Some(json) = text_map.get(text_id) {
        if let Ok(deltas) = serde_json::from_str::<Vec<TextDelta>>(json) {
          return Some(deltas);
        }
      }
    }
  }
  None
}

fn push_deltas_to_str(buf: &mut String, deltas: Vec<TextDelta>) {
  for delta in deltas {
    if let TextDelta::Inserted(text, _) = delta {
      let trimmed = text.trim();
      if !trimmed.is_empty() {
        buf.push_str(&trimmed);
        buf.push(' ');
      }
    }
  }
}

#[cfg(test)]
mod test {
  use crate::extract::document_to_plain_text;
  use workspace_template::document::get_started::{
    get_initial_document_data, get_started_document_data,
  };

  #[test]
  fn document_plain_text() {
    let doc = get_started_document_data().unwrap();
    let text = document_to_plain_text(&doc);
    let expected = "Welcome to AppFlowy! Here are the basics Click anywhere and just start typing. Highlight any text, and use the editing menu to style your writing however you like. As soon as you type / a menu will pop up. Select different types of content blocks you can add. Type / followed by /bullet or /num to create a list. Click + New Page button at the bottom of your sidebar to add a new page. Click + next to any page title in the sidebar to quickly add a new subpage, Document , Grid , or Kanban Board . Keyboard shortcuts, markdown, and code block Keyboard shortcuts guide Markdown reference Type /code to insert a code block // This is the main function.\nfn main() {\n    // Print text to the console.\n    println!(\"Hello World!\");\n} Have a question❓ Click ? at the bottom right for help and support. Like AppFlowy? Follow us: GitHub Twitter : @appflowy Newsletter ";
    assert_eq!(&text, expected);
  }

  #[test]
  fn document_plain_text_with_nested_blocks() {
    let doc = get_initial_document_data().unwrap();
    let text = document_to_plain_text(&doc);
    let expected = "Welcome to AppFlowy! Here are the basics Here is H3 Click anywhere and just start typing. Click Enter to create a new line. Highlight any text, and use the editing menu to style your writing however you like. As soon as you type / a menu will pop up. Select different types of content blocks you can add. Type / followed by /bullet or /num to create a list. Click + New Page button at the bottom of your sidebar to add a new page. Click + next to any page title in the sidebar to quickly add a new subpage, Document , Grid , or Kanban Board . Keyboard shortcuts, markdown, and code block Keyboard shortcuts guide Markdown reference Type /code to insert a code block // This is the main function.\nfn main() {\n    // Print text to the console.\n    println!(\"Hello World!\");\n} This is a paragraph This is a paragraph Have a question❓ Click ? at the bottom right for help and support. This is a paragraph This is a paragraph Click ? at the bottom right for help and support. Like AppFlowy? Follow us: GitHub Twitter : @appflowy Newsletter ";
    assert_eq!(&text, expected);
  }
}
