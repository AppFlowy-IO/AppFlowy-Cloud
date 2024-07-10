use serde::{Deserialize, Serialize};

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
  pub current_end_date: i64,
}

#[derive(Serialize, Deserialize)]
pub struct WorkspaceUsageAndLimit {
  pub member_count: i64,
  pub member_count_limit: i64,
  pub storage_bytes: i64,
  pub storage_bytes_limit: i64,
  pub storage_bytes_unlimited: bool,
  pub ai_responses_count: i64,
  pub ai_responses_count_limit: i64,

  pub local_ai: bool,
  pub ai_responses_unlimited: bool,
}
