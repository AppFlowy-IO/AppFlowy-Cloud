use app_error::AppError;
use async_openai::config::{AzureConfig, Config, OpenAIConfig};
use async_openai::types::{
  ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestUserMessageArgs,
  CreateChatCompletionRequestArgs, ResponseFormat, ResponseFormatJsonSchema,
};
use async_openai::Client;
use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{error, info, trace};
use uuid::Uuid;

pub enum AITool {
  OpenAI(OpenAIChat),
  AzureOpenAI(AzureOpenAIChat),
}

impl AITool {
  pub async fn summarize_documents(
    &self,
    question: &str,
    model_name: &str,
    documents: Vec<LLMDocument>,
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

#[derive(Debug)]
pub struct SearchSummary {
  pub content: String,
  pub highlights: String,
  pub sources: Vec<Uuid>,
  pub score: f32,
}

#[derive(Debug)]
pub struct SummarySearchResponse {
  pub summaries: Vec<SearchSummary>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SummarySearchSchema {
  pub answer: String,
  pub highlights: String,
  pub score: String,
  pub sources: Vec<String>,
}

const SYSTEM_PROMPT: &str = r#"
You are a concise, intelligent question-answering assistant.

Instructions:
- Use the provided context as the **primary basis** for your answer.
- You **may incorporate relevant knowledge** only if it supports or enhances the context meaningfully.
- The answer must be a **clear and concise**.

Output must include:
- `answer`: a concise summary.
- `highlights`:A markdown bullet list that highlights key themes and important details (e.g., date, time, location, etc.).
- `score`: relevance score (0.0–1.0), where:
  - 1.0 = fully supported by context,
  - 0.0 = unsupported,
  - values in between reflect partial support.
- `sources`: array of source IDs used for the answer.
"#;

const ONLY_CONTEXT_SYSTEM_PROMPT: &str = r#"
You are a strict, context-bound question answering assistant. Answer solely based on the text provided below. If the context lacks sufficient information for a confident response, reply with an empty answer.

Output must include:
- `answer`: a concise answer.
- `highlights`:A markdown bullet list that highlights key themes and important details (e.g., date, time, location, etc.).
- `score`: relevance score (0.0–1.0), where:
  - 1.0 = fully supported by context,
  - 0.0 = unsupported,
  - values in between reflect partial support.
- `sources`: array of source IDs used for the answer.

Do not reference or use any information beyond what is provided in the context.
"#;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LLMDocument {
  pub content: String,
  pub object_id: Uuid,
}

impl LLMDocument {
  pub fn new(content: String, object_id: Uuid) -> Self {
    Self { content, object_id }
  }
}

fn convert_documents_to_text(documents: Vec<LLMDocument>) -> String {
  documents
    .into_iter()
    .map(|doc| json!(doc).to_string())
    .collect::<Vec<String>>()
    .join("\n")
}

pub async fn summarize_documents<C: Config>(
  client: &Client<C>,
  question: &str,
  model_name: &str,
  documents: Vec<LLMDocument>,
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

  let schema = schema_for!(SummarySearchSchema);
  let schema_value = serde_json::to_value(&schema)?;
  let response_format = ResponseFormat::JsonSchema {
    json_schema: ResponseFormatJsonSchema {
      description: Some(
        "A response containing a final answer, highlight, score and relevance sources".to_string(),
      ),
      name: "SummarySearchSchema".into(),
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

  let response = client
    .chat()
    .create(request)
    .await?
    .choices
    .first()
    .and_then(|choice| choice.message.content.clone())
    .and_then(|content| serde_json::from_str::<SummarySearchSchema>(&content).ok())
    .ok_or_else(|| AppError::Unhandled("No response from OpenAI".to_string()))?;

  trace!("AI summary search document response: {:?}", response);
  if response.answer.is_empty() {
    return Ok(SummarySearchResponse { summaries: vec![] });
  }

  let score = match response.score.parse::<f32>() {
    Ok(score) => score,
    Err(err) => {
      error!(
        "[Search] Failed to parse AI summary score: {}. Error: {}",
        response.score, err
      );
      0.0
    },
  };

  // If only_context is true, we need to ensure the score is above a certain threshold.
  if only_context && score < 0.4 {
    info!(
      "[Search] AI summary score is too low: {}. Returning empty result.",
      score
    );
    return Ok(SummarySearchResponse { summaries: vec![] });
  }

  let summary = SearchSummary {
    content: response.answer,
    highlights: response.highlights,
    sources: response
      .sources
      .into_iter()
      .flat_map(|s| Uuid::parse_str(&s).ok())
      .collect(),
    score,
  };

  Ok(SummarySearchResponse {
    summaries: vec![summary],
  })
}
