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

pub enum AIChat {
  OpenAI(OpenAIChat),
  AzureOpenAI(AzureOpenAIChat),
}

impl AIChat {
  pub async fn chat_with_documents(
    &self,
    question: &str,
    model_name: &str,
    documents: &[LLMDocument],
    only_context: bool,
  ) -> Result<ChatResponse, AppError> {
    trace!(
      "Using model:{} to answer question:{}, with {} documents, only_context:{}",
      model_name,
      question,
      documents.len(),
      only_context
    );
    match self {
      AIChat::OpenAI(client) => {
        chat_with_documents(
          &client.client,
          question,
          model_name,
          documents,
          only_context,
        )
        .await
      },
      AIChat::AzureOpenAI(client) => {
        chat_with_documents(
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
pub struct ChatResponse {
  pub answers: Vec<String>,
  pub metadata: Value,
  pub score: String,
}

const SYSTEM_PROMPT: &str = r#"
You are a strict, context-based question answering assistant.
Provide only the answer text with empty json metadata.
At the end of your answer, include a numeric relevance score (0.0 to 1.0):
   - 1.0: Completely relevant.
   - 0.0: Not relevant.
   - Intermediate values indicate partial relevance.
"#;
const ONLY_CONTEXT_SYSTEM_PROMPT: &str = r#"
You are a strict, context-bound question answering assistant. Answer solely based on the text provided below. If the context lacks sufficient information for a confident response, reply with nothing.

Your response must include:
1. The answer text.
2. Metadata extracted from the context. (If the answer is not relevant, return an empty JSON map.)
3. A numeric score (0.0 to 1.0) indicating the answer's relevance to the user's question:
   - 1.0: Completely relevant.
   - 0.0: Not relevant at all.
   - Intermediate values indicate partial relevance.

Do not reference or use any information beyond what is provided in the context.
"#;

#[derive(Debug, Serialize)]
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

pub async fn chat_with_documents<C: Config>(
  client: &Client<C>,
  question: &str,
  model_name: &str,
  documents: &[LLMDocument],
  only_context: bool,
) -> Result<ChatResponse, AppError> {
  let documents_text = convert_documents_to_text(documents);
  let context = if only_context {
    format!(
      "{}\n\n##Context##\n{}",
      ONLY_CONTEXT_SYSTEM_PROMPT, documents_text
    )
  } else {
    SYSTEM_PROMPT.to_string()
  };

  let schema = schema_for!(ChatResponse);
  let schema_value = serde_json::to_value(&schema)?;
  let response_format = ResponseFormat::JsonSchema {
    json_schema: ResponseFormatJsonSchema {
      description: Some("the score indicating the relevance of answer to user question, the metadata is provided as a JSON map extracted directly from the context".to_string()),
      name: "ChatResponse".into(),
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

  let response = client.chat().create(request).await?;

  response
    .choices
    .first()
    .and_then(|choice| choice.message.content.clone())
    .and_then(|content| serde_json::from_str::<ChatResponse>(&content).ok())
    .ok_or_else(|| AppError::Unhandled("No response from OpenAI".to_string()))
}
