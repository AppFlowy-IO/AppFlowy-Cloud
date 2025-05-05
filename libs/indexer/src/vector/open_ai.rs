use app_error::AppError;
use appflowy_ai_client::dto::EmbeddingModel;
use async_openai::config::{AzureConfig, Config, OpenAIConfig};
use async_openai::types::{CreateEmbeddingRequest, CreateEmbeddingResponse};
use async_openai::Client;
use text_splitter::{ChunkConfig, TextSplitter};
use tiktoken_rs::CoreBPE;
use tracing::{trace, warn};

pub const OPENAI_EMBEDDINGS_URL: &str = "https://api.openai.com/v1/embeddings";

pub const REQUEST_PARALLELISM: usize = 40;

#[derive(Debug, Clone)]
pub struct OpenAIEmbedder {
  pub(crate) client: Client<OpenAIConfig>,
}

impl OpenAIEmbedder {
  pub fn new(config: OpenAIConfig) -> Self {
    let client = Client::with_config(config);

    Self { client }
  }
}

#[derive(Debug, Clone)]
pub struct AzureOpenAIEmbedder {
  pub(crate) client: Client<AzureConfig>,
}

impl AzureOpenAIEmbedder {
  pub fn new(mut config: AzureConfig) -> Self {
    // Make sure your Azure AI service support the model
    config = config.with_deployment_id(EmbeddingModel::default_model().to_string());
    let client = Client::with_config(config);
    Self { client }
  }
}

pub async fn async_embed<C: Config>(
  client: &Client<C>,
  request: CreateEmbeddingRequest,
) -> Result<CreateEmbeddingResponse, AppError> {
  trace!(
    "async embed with request: model:{:?}, dimension:{:?}, api_base:{}",
    request.model,
    request.dimensions,
    client.config().api_base()
  );
  let response = client
    .embeddings()
    .create(request)
    .await
    .map_err(|err| AppError::Unhandled(err.to_string()))?;
  Ok(response)
}

/// ## Execution Time Comparison Results
///
/// The following results were observed when running `execution_time_comparison_tests`:
///
/// | Content Size (chars) | Direct Time (ms) | spawn_blocking Time (ms) |
/// |-----------------------|------------------|--------------------------|
/// | 500                  | 1                | 1                        |
/// | 1000                 | 2                | 2                        |
/// | 2000                 | 5                | 5                        |
/// | 5000                 | 11               | 11                       |
/// | 20000                | 49               | 48                       |
///
/// ## Guidelines for Using `spawn_blocking`
///
/// - **Short Tasks (< 1 ms)**:
///   Use direct execution on the async runtime. The minimal execution time has negligible impact.
///
/// - **Moderate Tasks (1–10 ms)**:
///   - For infrequent or low-concurrency tasks, direct execution is acceptable.
///   - For frequent or high-concurrency tasks, consider using `spawn_blocking` to avoid delays.
///
/// - **Long Tasks (> 10 ms)**:
///   Always offload to a blocking thread with `spawn_blocking` to maintain runtime efficiency and responsiveness.
///
/// Related blog:
/// https://tokio.rs/blog/2020-04-preemption
/// https://ryhl.io/blog/async-what-is-blocking/
#[inline]
#[allow(dead_code)]
pub fn split_text_by_max_tokens(
  content: String,
  max_tokens: usize,
  tokenizer: &CoreBPE,
) -> Result<Vec<String>, AppError> {
  if content.is_empty() {
    return Ok(vec![]);
  }

  let token_ids = tokenizer.encode_ordinary(&content);
  let total_tokens = token_ids.len();
  if total_tokens <= max_tokens {
    return Ok(vec![content]);
  }

  let mut chunks = Vec::new();
  let mut start_idx = 0;
  while start_idx < total_tokens {
    let mut end_idx = (start_idx + max_tokens).min(total_tokens);
    let mut decoded = false;
    // Try to decode the chunk, adjust end_idx if decoding fails
    while !decoded {
      let token_chunk = &token_ids[start_idx..end_idx];
      // Attempt to decode the current chunk
      match tokenizer.decode(token_chunk.to_vec()) {
        Ok(chunk_text) => {
          chunks.push(chunk_text);
          start_idx = end_idx;
          decoded = true;
        },
        Err(_) => {
          // If we can extend the chunk, do so
          if end_idx < total_tokens {
            end_idx += 1;
          } else if start_idx + 1 < total_tokens {
            // Skip the problematic token at start_idx
            start_idx += 1;
            end_idx = (start_idx + max_tokens).min(total_tokens);
          } else {
            // Cannot decode any further, break to avoid infinite loop
            start_idx = total_tokens;
            break;
          }
        },
      }
    }
  }

  Ok(chunks)
}

/// Groups a list of paragraphs into chunks that fit within a specified maximum content length.
///
/// takes a vector of paragraph strings and combines them into larger chunks,
/// ensuring that each chunk's total byte length does not exceed the given `context_size`.
/// Paragraphs are concatenated directly without additional separators. If a single paragraph
/// exceeds the `context_size`, it is included as its own chunk without truncation.
///
/// # Arguments
/// * `paragraphs` - A vector of strings, where each string represents a paragraph.
/// * `context_size` - The maximum byte length allowed for each chunk.
///
/// # Returns
/// A vector of strings, where each string is a chunk of concatenated paragraphs that fits
/// within the `context_size`. If the input is empty, returns an empty vector.
pub fn group_paragraphs_by_max_content_len(
  paragraphs: Vec<String>,
  mut context_size: usize,
  overlap: usize,
) -> Vec<String> {
  if paragraphs.is_empty() {
    return vec![];
  }

  let mut result = Vec::new();
  let mut current = String::with_capacity(context_size.min(4096));

  if overlap > context_size {
    warn!("context_size is smaller than overlap, which may lead to unexpected behavior.");
    context_size = 2 * overlap;
  }

  let chunk_config = ChunkConfig::new(context_size)
    .with_overlap(overlap)
    .unwrap();
  let splitter = TextSplitter::new(chunk_config);

  for paragraph in paragraphs {
    if current.len() + paragraph.len() > context_size {
      if !current.is_empty() {
        result.push(std::mem::take(&mut current));
      }

      if paragraph.len() > context_size {
        let paragraph_chunks = splitter.chunks(&paragraph);
        result.extend(paragraph_chunks.map(String::from));
      } else {
        current.push_str(&paragraph);
      }
    } else {
      // Add paragraph to current chunk
      current.push_str(&paragraph);
    }
  }

  if !current.is_empty() {
    result.push(current);
  }

  result
}

#[cfg(test)]
mod tests {

  use crate::vector::open_ai::{group_paragraphs_by_max_content_len, split_text_by_max_tokens};
  use tiktoken_rs::cl100k_base;

  #[test]
  fn test_split_at_non_utf8() {
    let max_tokens = 10; // Small number for testing

    // Content with multibyte characters (emojis)
    let content = "Hello 😃 World 🌍! This is a test 🚀.".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = split_text_by_max_tokens(content.clone(), max_tokens, &tokenizer).unwrap();
    for content in params {
      assert!(content.is_char_boundary(0));
      assert!(content.is_char_boundary(content.len()));
    }

    let params = group_paragraphs_by_max_content_len(vec![content], max_tokens, 500);
    for content in params {
      assert!(content.is_char_boundary(0));
      assert!(content.is_char_boundary(content.len()));
    }
  }
  #[test]
  fn test_exact_boundary_split() {
    let max_tokens = 5; // Set to 5 tokens for testing
    let content = "The quick brown fox jumps over the lazy dog".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = split_text_by_max_tokens(content.clone(), max_tokens, &tokenizer).unwrap();

    let total_tokens = tokenizer.encode_ordinary(&content).len();
    let expected_fragments = total_tokens.div_ceil(max_tokens);
    assert_eq!(params.len(), expected_fragments);
  }

  #[test]
  fn test_content_shorter_than_max_len() {
    let max_tokens = 100;
    let content = "Short content".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = split_text_by_max_tokens(content.clone(), max_tokens, &tokenizer).unwrap();

    assert_eq!(params.len(), 1);
    assert_eq!(params[0], content);
  }

  #[test]
  fn test_empty_content() {
    let max_tokens = 10;
    let content = "".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = split_text_by_max_tokens(content.clone(), max_tokens, &tokenizer).unwrap();
    assert_eq!(params.len(), 0);

    let params = group_paragraphs_by_max_content_len(params, max_tokens, 500);
    assert_eq!(params.len(), 0);
  }

  #[test]
  fn test_content_with_only_multibyte_characters() {
    let max_tokens = 1; // Set to 1 token for testing
    let content = "😀😃😄😁😆".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = split_text_by_max_tokens(content.clone(), max_tokens, &tokenizer).unwrap();

    let emojis: Vec<String> = content.chars().map(|c| c.to_string()).collect();
    for (param, emoji) in params.iter().zip(emojis.iter()) {
      assert_eq!(param, emoji);
    }
  }

  #[test]
  fn test_split_with_combining_characters() {
    let max_tokens = 1; // Set to 1 token for testing
    let content = "a\u{0301}e\u{0301}i\u{0301}o\u{0301}u\u{0301}".to_string(); // "áéíóú"

    let tokenizer = cl100k_base().unwrap();
    let params = split_text_by_max_tokens(content.clone(), max_tokens, &tokenizer).unwrap();
    let total_tokens = tokenizer.encode_ordinary(&content).len();
    assert_eq!(params.len(), total_tokens);
    let reconstructed_content = params.join("");
    assert_eq!(reconstructed_content, content);

    let params = group_paragraphs_by_max_content_len(params, max_tokens, 500);
    let reconstructed_content: String = params.concat();
    assert_eq!(reconstructed_content, content);
  }

  #[test]
  fn test_large_content() {
    let max_tokens = 1000;
    let content = "a".repeat(5000); // 5000 characters
    let tokenizer = cl100k_base().unwrap();
    let params = split_text_by_max_tokens(content.clone(), max_tokens, &tokenizer).unwrap();

    let total_tokens = tokenizer.encode_ordinary(&content).len();
    let expected_fragments = total_tokens.div_ceil(max_tokens);
    assert_eq!(params.len(), expected_fragments);
  }

  #[test]
  fn test_non_ascii_characters() {
    let max_tokens = 2;
    let content = "áéíóú".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = split_text_by_max_tokens(content.clone(), max_tokens, &tokenizer).unwrap();

    let total_tokens = tokenizer.encode_ordinary(&content).len();
    let expected_fragments = total_tokens.div_ceil(max_tokens);
    assert_eq!(params.len(), expected_fragments);
    let reconstructed_content: String = params.concat();
    assert_eq!(reconstructed_content, content);

    let params = group_paragraphs_by_max_content_len(params, max_tokens, 500);
    let reconstructed_content: String = params.concat();
    assert_eq!(reconstructed_content, content);
  }

  #[test]
  fn test_content_with_leading_and_trailing_whitespace() {
    let max_tokens = 3;
    let content = "  abcde  ".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = split_text_by_max_tokens(content.clone(), max_tokens, &tokenizer).unwrap();

    let total_tokens = tokenizer.encode_ordinary(&content).len();
    let expected_fragments = total_tokens.div_ceil(max_tokens);
    assert_eq!(params.len(), expected_fragments);
    let reconstructed_content: String = params.concat();
    assert_eq!(reconstructed_content, content);

    let params = group_paragraphs_by_max_content_len(params, max_tokens, 10);
    let reconstructed_content: String = params.concat();
    assert_eq!(reconstructed_content, content);
  }

  #[test]
  fn test_content_with_multiple_zero_width_joiners() {
    let max_tokens = 1;
    let content = "👩‍👩‍👧‍👧👨‍👨‍👦‍👦".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = split_text_by_max_tokens(content.clone(), max_tokens, &tokenizer).unwrap();
    let reconstructed_content: String = params.concat();
    assert_eq!(reconstructed_content, content);

    let params = group_paragraphs_by_max_content_len(params, max_tokens, 10);
    let reconstructed_content: String = params.concat();
    assert_eq!(reconstructed_content, content);
  }

  #[test]
  fn test_content_with_long_combining_sequences() {
    let max_tokens = 1;
    let content = "a\u{0300}\u{0301}\u{0302}\u{0303}\u{0304}".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = split_text_by_max_tokens(content.clone(), max_tokens, &tokenizer).unwrap();
    let reconstructed_content: String = params.concat();
    assert_eq!(reconstructed_content, content);

    let params = group_paragraphs_by_max_content_len(params, max_tokens, 10);
    let reconstructed_content: String = params.concat();
    assert_eq!(reconstructed_content, content);
  }

  #[test]
  fn test_multiple_paragraphs_single_chunk() {
    let paragraphs = vec![
      "First paragraph.".to_string(),
      "Second paragraph.".to_string(),
      "Third paragraph.".to_string(),
    ];
    let result = group_paragraphs_by_max_content_len(paragraphs, 100, 5);
    assert_eq!(result.len(), 1);
    assert_eq!(
      result[0],
      "First paragraph.Second paragraph.Third paragraph."
    );
  }

  #[test]
  fn test_large_paragraph_splitting() {
    // Create a paragraph larger than context size
    let large_paragraph = "A".repeat(50);
    let paragraphs = vec![
      "Small paragraph.".to_string(),
      large_paragraph.clone(),
      "Another small one.".to_string(),
    ];

    let result = group_paragraphs_by_max_content_len(paragraphs, 30, 10);

    // Expected: "Small paragraph." as one chunk, then multiple chunks for the large paragraph,
    // then "Another small one." as the final chunk
    assert!(result.len() > 3); // At least 4 chunks (1 + at least 2 for large + 1)
    assert_eq!(result[0], "Small paragraph.");
    assert!(result[1].starts_with("A"));

    // Check that all chunks of the large paragraph together contain the original text
    let large_chunks = &result[1..result.len() - 1];
    let reconstructed = large_chunks.join("");
    // Due to overlaps, the reconstructed text might be longer
    assert!(large_paragraph.len() <= reconstructed.len());
    assert!(reconstructed.chars().all(|c| c == 'A'));
  }

  #[test]
  fn test_overlap_larger_than_context_size() {
    let paragraphs = vec![
      "First paragraph.".to_string(),
      "Second very long paragraph that needs to be split.".to_string(),
    ];

    // Overlap larger than context size
    let result = group_paragraphs_by_max_content_len(paragraphs, 10, 20);

    // Check that the function didn't panic and produced reasonable output
    assert!(!result.is_empty());
    assert_eq!(result[0], "First paragraph.");
    assert_eq!(result[1], "Second very long paragraph that needs to");
    assert_eq!(result[2], "that needs to be split.");

    assert!(result.iter().all(|chunk| chunk.len() <= 40));
  }

  #[test]
  fn test_exact_fit() {
    let paragraph1 = "AAAA".to_string(); // 4 bytes
    let paragraph2 = "BBBB".to_string(); // 4 bytes
    let paragraph3 = "CCCC".to_string(); // 4 bytes

    let paragraphs = vec![paragraph1, paragraph2, paragraph3];

    // Context size exactly fits 2 paragraphs
    let result = group_paragraphs_by_max_content_len(paragraphs, 8, 2);

    assert_eq!(result.len(), 2);
    assert_eq!(result[0], "AAAABBBB");
    assert_eq!(result[1], "CCCC");
  }
  #[test]
  fn test_edit_paragraphs() {
    // Create initial paragraphs and convert them to Strings.
    let mut paragraphs = vec![
      "Rust is a multiplayer survival game developed by Facepunch Studios,",
      "Rust is a modern, system-level programming language designed with a focus on performance, safety, and concurrency. ",
      "Rust as a Natural Process (Oxidation) refers to the chemical reaction that occurs when metals, primarily iron, come into contact with oxygen and moisture (water) over time, leading to the formation of iron oxide, commonly known as rust. This process is a form of oxidation, where a substance reacts with oxygen in the air or water, resulting in the degradation of the metal.",
    ]
        .into_iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();

    // Confirm the original lengths of the paragraphs.
    assert_eq!(paragraphs[0].len(), 67);
    assert_eq!(paragraphs[1].len(), 115);
    assert_eq!(paragraphs[2].len(), 374);

    // First grouping of paragraphs with a context size of 200 and overlap 100.
    // Expecting 4 chunks based on the original paragraphs.
    let result = group_paragraphs_by_max_content_len(paragraphs.clone(), 200, 100);
    assert_eq!(result.len(), 4);

    // Edit paragraph 0 by appending more text, simulating a content update.
    paragraphs.get_mut(0).unwrap().push_str(
      " first released in early access in December 2013 and fully launched in February 2018",
    );
    // After edit, confirm that paragraph 0's length is updated.
    assert_eq!(paragraphs[0].len(), 151);

    // Group paragraphs again after the edit.
    // The change should cause a shift in grouping, expecting 5 chunks now.
    let result_2 = group_paragraphs_by_max_content_len(paragraphs.clone(), 200, 100);
    assert_eq!(result_2.len(), 5);

    // Verify that parts of the original grouping (later chunks) remain the same:
    // The third chunk from the first run should equal the fourth from the second run.
    assert_eq!(result[2], result_2[3]);
    // The fourth chunk from the first run should equal the fifth from the second run.
    assert_eq!(result[3], result_2[4]);

    // Edit paragraph 1 by appending extra text, simulating another update.
    paragraphs
      .get_mut(1)
      .unwrap()
      .push_str("It was created by Mozilla.");

    // Group paragraphs once again after the second edit.
    let result_3 = group_paragraphs_by_max_content_len(paragraphs.clone(), 200, 100);
    assert_eq!(result_3.len(), 5);

    // Confirm that the grouping for the unchanged parts is still consistent:
    // The first chunk from the previous grouping (before editing paragraph 1) stays the same.
    assert_eq!(result_2[0], result_3[0]);
    // Similarly, the second and third chunks (from result_2) remain unchanged.
    assert_eq!(result_2[2], result_3[2]);
    // The fourth chunk in both groupings should still be identical.
    assert_eq!(result_2[3], result_3[3]);
    // And the fifth chunk in both groupings is compared for consistency.
    assert_eq!(result_2[4], result_3[4]);
  }
}
