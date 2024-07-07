use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RecurringInterval {
  Month,
  Year,
}

impl RecurringInterval {
  pub fn as_str(&self) -> &str {
    match self {
      RecurringInterval::Month => "month",
      RecurringInterval::Year => "year",
    }
  }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
#[repr(i16)]
pub enum SubscriptionPlan {
  Unknown = -1,

  Free = 0,
  Pro = 1,
  Team = 2,

  AIMax = 3,
  AILocal = 4,
}

impl AsRef<str> for SubscriptionPlan {
  fn as_ref(&self) -> &str {
    match self {
      SubscriptionPlan::Free => "free",
      SubscriptionPlan::Pro => "pro",
      SubscriptionPlan::Team => "team",
      SubscriptionPlan::AIMax => "ai_max",
      SubscriptionPlan::AILocal => "ai_local",
      SubscriptionPlan::Unknown => "unknown",
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

#[derive(Deserialize, Debug)]
pub struct WorkspaceSubscriptionStatus {
  pub workspace_id: String,
  pub workspace_plan: SubscriptionPlan,
  pub recurring_interval: RecurringInterval,
  pub subscription_status: SubscriptionStatus,
  pub subscription_quantity: u64,
  pub canceled_at: Option<i64>,
}

#[derive(Deserialize)]
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
