use app_error::AppError;
use async_openai::config::{AzureConfig, Config, OpenAIConfig};
use async_openai::types::{
  ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestUserMessageArgs,
  CreateChatCompletionRequestArgs, ResponseFormat, ResponseFormatJsonSchema,
};
use async_openai::Client;
use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::trace;

pub enum AITool {
  OpenAI(OpenAIChat),
  AzureOpenAI(AzureOpenAIChat),
}

impl AITool {
  pub async fn summary_documents(
    &self,
    question: &str,
    model_name: &str,
    documents: &[LLMDocument],
    only_context: bool,
  ) -> Result<SummarySearchResponse, AppError> {
    trace!(
      "Using model:{} to answer question:{}, with {} documents, only_context:{}",
      model_name,
      question,
      documents.len(),
      only_context
    );
    match self {
      AITool::OpenAI(client) => {
        summarize_documents(
          &client.client,
          question,
          model_name,
          documents,
          only_context,
        )
        .await
      },
      AITool::AzureOpenAI(client) => {
        summarize_documents(
          &client.client,
          question,
          model_name,
          documents,
          only_context,
        )
        .await
      },
    }
  }
}

pub struct OpenAIChat {
  pub client: Client<OpenAIConfig>,
}

impl OpenAIChat {
  pub fn new(config: OpenAIConfig) -> Self {
    let client = Client::with_config(config);
    Self { client }
  }
}

pub struct AzureOpenAIChat {
  pub client: Client<AzureConfig>,
}

impl AzureOpenAIChat {
  pub fn new(config: AzureConfig) -> Self {
    let client = Client::with_config(config);
    Self { client }
  }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchSummary {
  pub content: String,
  pub metadata: Value,
  pub score: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SummarySearchResponse {
  pub summaries: Vec<SearchSummary>,
}

const SYSTEM_PROMPT: &str = r#"
You are a strict, context-based question answering assistant.
Provide an answer with appropriate metadata in JSON format.
For each answer, include:
1. The answer text. It must be concise summaries and highly precise.
2. Metadata with empty JSON map
3. A numeric relevance score (0.0 to 1.0):
   - 1.0: Completely relevant.
   - 0.0: Not relevant.
   - Intermediate values indicate partial relevance.
"#;
const ONLY_CONTEXT_SYSTEM_PROMPT: &str = r#"
You are a strict, context-bound question answering assistant. Answer solely based on the text provided below. If the context lacks sufficient information for a confident response, reply with an empty answer.

Your response must include:
1. The answer text. It must be concise summaries and highly precise.
2. Metadata extracted from the context. (If the answer is not relevant, return an empty JSON Map.)
3. A numeric score (0.0 to 1.0) indicating the answer's relevance to the user's question:
   - 1.0: Completely relevant.
   - 0.0: Not relevant at all.
   - Intermediate values indicate partial relevance.

Do not reference or use any information beyond what is provided in the context.
"#;

#[derive(Debug, Serialize, Deserialize)]
pub struct LLMDocument {
  pub content: String,
  pub metadata: Value,
}

impl LLMDocument {
  pub fn new(content: String, metadata: Value) -> Self {
    Self { content, metadata }
  }
}

fn convert_documents_to_text(documents: &[LLMDocument]) -> String {
  documents
    .iter()
    .map(|doc| json!(doc).to_string())
    .collect::<Vec<String>>()
    .join("\n")
}

pub async fn summarize_documents<C: Config>(
  client: &Client<C>,
  question: &str,
  model_name: &str,
  documents: &[LLMDocument],
  only_context: bool,
) -> Result<SummarySearchResponse, AppError> {
  let documents_text = convert_documents_to_text(documents);
  let context = if only_context {
    format!(
      "{}\n\n##Context##\n{}",
      ONLY_CONTEXT_SYSTEM_PROMPT, documents_text
    )
  } else {
    SYSTEM_PROMPT.to_string()
  };

  let schema = schema_for!(SummarySearchResponse);
  let schema_value = serde_json::to_value(&schema)?;
  let response_format = ResponseFormat::JsonSchema {
    json_schema: ResponseFormatJsonSchema {
      description: Some("A response containing a list of answers, each with the answer text, metadata extracted from context, and relevance score".to_string()),
      name: "SummarySearchResponse".into(),
      schema: Some(schema_value),
      strict: Some(true),
    },
  };

  let request = CreateChatCompletionRequestArgs::default()
    .model(model_name)
    .messages([
      ChatCompletionRequestSystemMessageArgs::default()
        .content(context)
        .build()?
        .into(),
      ChatCompletionRequestUserMessageArgs::default()
        .content(question)
        .build()?
        .into(),
    ])
    .response_format(response_format)
    .build()?;

  let mut response = client
    .chat()
    .create(request)
    .await?
    .choices
    .first()
    .and_then(|choice| choice.message.content.clone())
    .and_then(|content| serde_json::from_str::<SummarySearchResponse>(&content).ok())
    .ok_or_else(|| AppError::Unhandled("No response from OpenAI".to_string()))?;

  // Remove empty summaries
  response
    .summaries
    .retain(|summary| !summary.content.is_empty());

  Ok(response)
}
