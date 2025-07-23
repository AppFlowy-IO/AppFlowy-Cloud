use crate::{process_response_data, process_response_error, Client};
use client_api_entity::billing_dto::{
  SetSubscriptionRecurringInterval, SubscriptionCancelRequest, SubscriptionLinkRequest,
  SubscriptionPlanDetail, WorkspaceUsageAndLimit,
};
use reqwest::Method;
use shared_entity::{
  dto::billing_dto::{RecurringInterval, SubscriptionPlan, WorkspaceSubscriptionStatus},
  response::{AppResponse, AppResponseError},
};

lazy_static::lazy_static! {
  static ref BASE_BILLING_URL: Option<String> = match std::env::var("APPFLOWY_CLOUD_BASE_BILLING_URL") {
    Ok(url) => Some(url),
    Err(err) => {
      tracing::warn!("std::env::var(APPFLOWY_CLOUD_BASE_BILLING_URL): {}", err);
      None
    },
  };
}

impl Client {
  pub fn base_billing_url(&self) -> &str {
    BASE_BILLING_URL.as_deref().unwrap_or(&self.base_url)
  }

  pub async fn customer_id(&self) -> Result<String, AppResponseError> {
    let url = format!("{}/billing/api/v1/customer-id", self.base_billing_url());
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    process_response_data::<String>(resp).await
  }

  pub async fn create_subscription(
    &self,
    workspace_id: &str,
    recurring_interval: RecurringInterval,
    workspace_subscription_plan: SubscriptionPlan,
    success_url: &str,
  ) -> Result<String, AppResponseError> {
    let sub_link_req = SubscriptionLinkRequest {
      workspace_subscription_plan,
      recurring_interval,
      workspace_id: workspace_id.to_string(),
      success_url: success_url.to_string(),
      with_test_clock: None,
    };

    self.create_subscription_v2(&sub_link_req).await
  }

  pub async fn create_subscription_v2(
    &self,
    sub_link_req: &SubscriptionLinkRequest,
  ) -> Result<String, AppResponseError> {
    let url = format!(
      "{}/billing/api/v1/subscription-link",
      self.base_billing_url()
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .query(sub_link_req)
      .send()
      .await?;

    process_response_data::<String>(resp).await
  }

  pub async fn cancel_subscription(
    &self,
    req: &SubscriptionCancelRequest,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/billing/api/v1/cancel-subscription",
      self.base_billing_url()
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(req)
      .send()
      .await?;
    process_response_error(resp).await
  }

  pub async fn list_subscription(
    &self,
  ) -> Result<Vec<WorkspaceSubscriptionStatus>, AppResponseError> {
    let url = format!(
      "{}/billing/api/v1/subscription-status",
      self.base_billing_url(),
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;

    process_response_data::<Vec<WorkspaceSubscriptionStatus>>(resp).await
  }

  pub async fn get_portal_session_link(&self) -> Result<String, AppResponseError> {
    let url = format!(
      "{}/billing/api/v1/portal-session-link",
      self.base_billing_url()
    );
    let portal_url = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?
      .error_for_status()?
      .json::<AppResponse<String>>()
      .await?
      .into_data()?;
    Ok(portal_url)
  }

  pub async fn get_workspace_usage_and_limit(
    &self,
    workspace_id: &str,
  ) -> Result<WorkspaceUsageAndLimit, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/usage-and-limit",
      self.base_url, workspace_id
    );
    self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?
      .error_for_status()?
      .json::<AppResponse<WorkspaceUsageAndLimit>>()
      .await?
      .into_data()
  }

  /// Query all subscription status for a workspace
  pub async fn get_workspace_subscriptions(
    &self,
    workspace_id: &str,
  ) -> Result<Vec<WorkspaceSubscriptionStatus>, AppResponseError> {
    let url = format!(
      "{}/billing/api/v1/subscription-status/{}",
      self.base_billing_url(),
      workspace_id
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;

    process_response_data::<Vec<WorkspaceSubscriptionStatus>>(resp).await
  }

  /// Query all active subscription, minimal information but faster
  pub async fn get_active_workspace_subscriptions(
    &self,
    workspace_id: &str,
  ) -> Result<Vec<SubscriptionPlan>, AppResponseError> {
    let url = format!(
      "{}/billing/api/v1/active-subscription/{}",
      self.base_billing_url(),
      workspace_id
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;

    process_response_data::<Vec<SubscriptionPlan>>(resp).await
  }

  /// Set subscription recurring interval
  pub async fn set_subscription_recurring_interval(
    &self,
    set_sub_recur: &SetSubscriptionRecurringInterval,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/billing/api/v1/subscription-recurring-interval",
      self.base_billing_url(),
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(set_sub_recur)
      .send()
      .await?;

    process_response_error(resp).await
  }

  /// get all subscription plan details
  pub async fn get_subscription_plan_details(
    &self,
  ) -> Result<Vec<SubscriptionPlanDetail>, AppResponseError> {
    let url = format!("{}/billing/api/v1/subscriptions", self.base_billing_url(),);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;

    process_response_data::<Vec<SubscriptionPlanDetail>>(resp).await
  }
}
