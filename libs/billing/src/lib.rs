pub mod entities;
use crate::entities::WorkspaceUsageLimit;
use client_api::error::AppResponseError;
use entities::{RecurringInterval, SubscriptionPlan, WorkspaceSubscriptionStatus, WorkspaceUsage};
use reqwest::Method;
use serde_json::json;
use shared_entity::response::AppResponse;

pub struct BillingClient<'a> {
    billing_base_url: String,
    client: &'a client_api::Client,
}

impl<'a> From<&'a client_api::Client> for BillingClient<'a> {
    fn from(client: &'a client_api::Client) -> Self {
        Self {
            billing_base_url: client.base_url.clone(),
            client,
        }
    }
}

impl BillingClient<'_> {
    pub fn set_billing_base_url(&mut self, billing_base_url: String) {
        self.billing_base_url = billing_base_url;
    }

    pub async fn customer_id(&self) -> Result<String, AppResponseError> {
        let url = format!("{}/billing/api/v1/customer-id", &self.billing_base_url,);

        let resp = self
            .client
            .http_client_with_auth(Method::GET, &url)
            .await?
            .send()
            .await?;

        AppResponse::<String>::from_response(resp)
            .await?
            .into_data()
    }

    pub async fn create_subscription(
        &self,
        workspace_id: &str,
        recurring_interval: RecurringInterval,
        workspace_subscription_plan: SubscriptionPlan,
        success_url: &str,
    ) -> Result<String, AppResponseError> {
        let url = format!(
            "{}/billing/api/v1/subscription-link",
            &self.billing_base_url,
        );

        let resp = self
            .client
            .http_client_with_auth(Method::GET, &url)
            .await?
            .query(&[
                ("workspace_id", workspace_id),
                ("recurring_interval", recurring_interval.as_str()),
                (
                    "workspace_subscription_plan",
                    &workspace_subscription_plan.as_str(),
                ),
                ("success_url", success_url),
            ])
            .send()
            .await?;

        AppResponse::<String>::from_response(resp)
            .await?
            .into_data()
    }

    pub async fn cancel_subscription(&self, workspace_id: &str) -> Result<(), AppResponseError> {
        let url = format!(
            "{}/billing/api/v1/cancel-subscription",
            &self.billing_base_url,
        );
        let resp = self
            .client
            .http_client_with_auth(Method::POST, &url)
            .await?
            .json(&json!({ "workspace_id": workspace_id }))
            .send()
            .await?;
        AppResponse::<()>::from_response(resp).await?.into_error()
    }

    pub async fn list_subscription(
        &self,
    ) -> Result<Vec<WorkspaceSubscriptionStatus>, AppResponseError> {
        let url = format!(
            "{}/billing/api/v1/subscription-status",
            &self.billing_base_url
        );
        let resp = self
            .client
            .http_client_with_auth(Method::GET, &url)
            .await?
            .send()
            .await?;

        AppResponse::<Vec<WorkspaceSubscriptionStatus>>::from_response(resp)
            .await?
            .into_data()
    }

    pub async fn get_workspace_usage(
        &self,
        workspace_id: &str,
    ) -> Result<WorkspaceUsage, AppResponseError> {
        let num_members = self.client.get_workspace_members(workspace_id).await?.len();
        let limits = get_workspace_limits(self.client, workspace_id).await?;
        let doc_usage = self.client.get_workspace_usage(workspace_id).await?;

        let workspace_usage = WorkspaceUsage {
            member_count: num_members,
            member_count_limit: limits.member_count,
            total_blob_bytes: doc_usage.consumed_capacity as _,
            total_blob_bytes_limit: limits.total_blob_size,
        };
        Ok(workspace_usage)
    }

    pub async fn get_portal_session_link(&self) -> Result<String, AppResponseError> {
        let url = format!(
            "{}/billing/api/v1/portal-session-link",
            &self.billing_base_url,
        );
        let portal_url = self
            .client
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
}

async fn get_workspace_limits(
    client: &client_api::Client,
    workspace_id: &str,
) -> Result<WorkspaceUsageLimit, AppResponseError> {
    let url = format!("{}/api/workspace/{}/limit", &client.base_url, workspace_id);
    client
        .http_client_with_auth(Method::GET, &url)
        .await?
        .send()
        .await?
        .error_for_status()?
        .json::<AppResponse<WorkspaceUsageLimit>>()
        .await?
        .into_data()
}
