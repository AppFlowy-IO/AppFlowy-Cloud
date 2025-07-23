use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::Display;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RecurringInterval {
  Month = 0,
  Year = 1,
}

impl RecurringInterval {
  pub fn as_str(&self) -> &str {
    match self {
      RecurringInterval::Month => "month",
      RecurringInterval::Year => "year",
    }
  }
}

impl TryFrom<i16> for RecurringInterval {
  type Error = String;

  fn try_from(value: i16) -> Result<Self, Self::Error> {
    match value {
      0 => Ok(RecurringInterval::Month),
      1 => Ok(RecurringInterval::Year),
      _ => Err(format!("Invalid RecurringInterval value: {}", value)),
    }
  }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
#[repr(i16)]
pub enum SubscriptionPlan {
  Free = 0,
  Pro = 1,
  Team = 2,

  AiMax = 3,
  AiLocal = 4,
}

impl TryFrom<i16> for SubscriptionPlan {
  type Error = String;

  fn try_from(value: i16) -> Result<Self, Self::Error> {
    match value {
      0 => Ok(SubscriptionPlan::Free),
      1 => Ok(SubscriptionPlan::Pro),
      2 => Ok(SubscriptionPlan::Team),
      3 => Ok(SubscriptionPlan::AiMax),
      4 => Ok(SubscriptionPlan::AiLocal),
      _ => Err(format!("Invalid SubscriptionPlan value: {}", value)),
    }
  }
}

impl AsRef<str> for SubscriptionPlan {
  fn as_ref(&self) -> &str {
    match self {
      SubscriptionPlan::Free => "free",
      SubscriptionPlan::Pro => "pro",
      SubscriptionPlan::Team => "team",
      SubscriptionPlan::AiMax => "ai_max",
      SubscriptionPlan::AiLocal => "ai_local",
    }
  }
}

impl TryFrom<&str> for SubscriptionPlan {
  type Error = String;

  fn try_from(value: &str) -> Result<Self, Self::Error> {
    match value {
      "free" => Ok(SubscriptionPlan::Free),
      "pro" => Ok(SubscriptionPlan::Pro),
      "team" => Ok(SubscriptionPlan::Team),
      "ai_max" => Ok(SubscriptionPlan::AiMax),
      "ai_local" => Ok(SubscriptionPlan::AiLocal),
      _ => Err(format!("Invalid SubscriptionPlan value: {}", value)),
    }
  }
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionStatus {
  Active,
  Canceled,
  Incomplete,
  IncompleteExpired,
  PastDue,
  Paused,
  Trialing,
  Unpaid,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WorkspaceSubscriptionStatus {
  pub workspace_id: String,
  pub workspace_plan: SubscriptionPlan,
  pub recurring_interval: RecurringInterval,
  pub subscription_status: SubscriptionStatus,
  pub subscription_quantity: u64,
  pub cancel_at: Option<i64>,
  pub current_period_end: i64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WorkspaceUsageAndLimit {
  pub member_count: i64,
  pub member_count_limit: i64,
  pub storage_bytes: i64,
  pub storage_bytes_limit: i64,
  pub storage_bytes_unlimited: bool,
  pub single_upload_limit: i64,
  pub single_upload_unlimited: bool,
  pub ai_responses_count: i64,
  pub ai_responses_count_limit: i64,
  #[serde(default)]
  pub ai_image_responses_count: i64,
  #[serde(default)]
  pub ai_image_responses_count_limit: i64,

  pub local_ai: bool,
  pub ai_responses_unlimited: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SubscriptionCancelRequest {
  pub workspace_id: String,
  pub plan: SubscriptionPlan,
  pub sync: bool, // if true, this request will block until stripe has sent the cancellation webhook
  pub reason: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SetSubscriptionRecurringInterval {
  pub workspace_id: String,
  pub plan: SubscriptionPlan,
  pub recurring_interval: RecurringInterval,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SubscriptionPlanDetail {
  pub currency: Currency,
  pub price_cents: i64,
  pub recurring_interval: RecurringInterval,
  pub plan: SubscriptionPlan,
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize, Eq, PartialEq, Hash, Default)]
pub enum Currency {
  #[default]
  USD,
}

#[derive(Serialize, Deserialize)]
pub struct SubscriptionLinkRequest {
  pub workspace_subscription_plan: SubscriptionPlan,
  pub recurring_interval: RecurringInterval,
  pub workspace_id: String,
  pub success_url: String,
  pub with_test_clock: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SubscriptionTrialRequest {
  pub plan: SubscriptionPlan,
  #[serde(default)]
  #[serde(skip_serializing_if = "Option::is_none")]
  pub period_days: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LicensedProductDetail {
  pub name: String,
  pub currency: Currency,
  pub price_cents: i64,
  pub recurring_interval: RecurringInterval,
  pub product_type: LicensedProductType,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserSubscribeProduct {
  pub email: String,
  pub product_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeProductLicense {
  pub product_id: String,
  pub policy_id: String,
  pub license_id: String,
  pub metadata: serde_json::Value,
  pub max_machines: i32,
  pub unlimited_devices: bool,
  pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, serde_repr::Serialize_repr, serde_repr::Deserialize_repr)]
#[repr(u8)]
pub enum LicensedProductType {
  Unknown = 0,
  AppFlowyAI = 1,
  AppFlowyCloudPremium = 2,
}

impl Display for LicensedProductType {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", (self.clone() as u8))
  }
}

impl TryFrom<&str> for LicensedProductType {
  type Error = String;

  fn try_from(value: &str) -> Result<Self, Self::Error> {
    match value {
      "1" => Ok(LicensedProductType::AppFlowyAI),
      "2" => Ok(LicensedProductType::AppFlowyCloudPremium),
      _ => Err(format!("Invalid LicensedProductType value: {}", value)),
    }
  }
}

#[derive(Serialize, Deserialize)]
pub struct LicenseProductSubscriptionLinkQuery {
  pub product_type: LicensedProductType,
}
