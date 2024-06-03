use collab_document::blocks::{DocumentData, TextDelta};

pub fn document_to_plain_text(document: &DocumentData) -> String {
  let mut buf = String::new();
  if let Some(text_map) = document.meta.text_map.as_ref() {
    let mut stack = Vec::new();
    stack.push(&document.page_id);
    // do a depth-first scan of the document blocks
    while let Some(block_id) = stack.pop() {
      if let Some(block) = document.blocks.get(block_id) {
        if block.external_type.as_deref() == Some("text") {
          if let Some(text_id) = block.external_id.as_deref() {
            if let Some(json) = text_map.get(text_id) {
              match serde_json::from_str::<Vec<TextDelta>>(json) {
                Ok(deltas) => {
                  for delta in deltas {
                    if let TextDelta::Inserted(text, _) = delta {
                      buf.push_str(&text);
                    }
                  }
                },
                Err(err) => {
                  tracing::error!("text_id `{}` is not a valid delta array: {}", text_id, err)
                },
              }
            }
            buf.push('\n');
          }
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
  buf
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
    let expected = "\nWelcome to AppFlowy!\nHere are the basics\nClick anywhere and just start typing.\nHighlight any text, and use the editing menu to style your writing however you like.\nAs soon as you type / a menu will pop up. Select different types of content blocks you can add.\nType / followed by /bullet or /num to create a list.\nClick + New Page button at the bottom of your sidebar to add a new page.\nClick + next to any page title in the sidebar to quickly add a new subpage, Document, Grid, or Kanban Board.\n\n\nKeyboard shortcuts, markdown, and code block\nKeyboard shortcuts guide\nMarkdown reference\nType /code to insert a code block\n// This is the main function.\nfn main() {\n    // Print text to the console.\n    println!(\"Hello World!\");\n}\n\nHave a question❓\nClick ? at the bottom right for help and support.\n\n\nLike AppFlowy? Follow us:\nGitHub\nTwitter: @appflowy\nNewsletter\n\n\n\n\n";
    assert_eq!(&text, expected);
  }

  #[test]
  fn document_plain_text_with_nested_blocks() {
    let doc = get_initial_document_data().unwrap();
    let text = document_to_plain_text(&doc);
    let expected = "Welcome to AppFlowy!\nHere are the basics\nHere is H3\nClick anywhere and just start typing.\nClick Enter to create a new line.\nHighlight any text, and use the editing menu to style your writing however you like.\nAs soon as you type / a menu will pop up. Select different types of content blocks you can add.\nType / followed by /bullet or /num to create a list.\nClick + New Page button at the bottom of your sidebar to add a new page.\nClick + next to any page title in the sidebar to quickly add a new subpage, Document, Grid, or Kanban Board.\n\n\nKeyboard shortcuts, markdown, and code block\nKeyboard shortcuts guide\nMarkdown reference\nType /code to insert a code block\n// This is the main function.\nfn main() {\n    // Print text to the console.\n    println!(\"Hello World!\");\n}\n\nThis is a paragraph\nThis is a paragraph\nHave a question❓\nClick ? at the bottom right for help and support.\nThis is a paragraph\nThis is a paragraph\nClick ? at the bottom right for help and support.\n\n\nLike AppFlowy? Follow us:\nGitHub\nTwitter: @appflowy\nNewsletter\n\n\n\n\n";
    assert_eq!(&text, expected);
  }
}
