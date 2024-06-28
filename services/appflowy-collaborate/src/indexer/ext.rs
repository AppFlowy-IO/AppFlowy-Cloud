use collab_document::blocks::{DocumentData, TextDelta};

pub trait DocumentDataExt {
  fn to_plain_text(&self) -> String;
}

impl DocumentDataExt for DocumentData {
  fn to_plain_text(&self) -> String {
    let mut buf = String::new();
    if let Some(text_map) = self.meta.text_map.as_ref() {
      let mut stack = Vec::new();
      stack.push(&self.page_id);
      // do a depth-first scan of the document blocks
      while let Some(block_id) = stack.pop() {
        if let Some(block) = self.blocks.get(block_id) {
          if let Some(deltas) = get_delta_from_block_data(block) {
            push_deltas_to_str(&mut buf, deltas);
          } else if let Some(deltas) = get_delta_from_external_text_id(block, text_map) {
            push_deltas_to_str(&mut buf, deltas);
          }
          if let Some(children) = self.meta.children_map.get(&block.children) {
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
        buf.push_str(trimmed);
        buf.push(' ');
      }
    }
  }
}
