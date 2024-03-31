use crate::access::{
  load_group_policies, ObjectType, POLICY_FIELD_INDEX_OBJECT, POLICY_FIELD_INDEX_SUBJECT,
};
use crate::act::ActionVariant;
use crate::metrics::MetricsCalState;
use crate::request::{GroupPolicyRequest, PolicyRequest, WorkspacePolicyRequest};
use anyhow::anyhow;
use app_error::AppError;
use async_trait::async_trait;
use casbin::{CoreApi, Enforcer, MgmtApi};
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{event, instrument, trace};

pub const ENFORCER_METRICS_TICK_INTERVAL: Duration = Duration::from_secs(120);

#[async_trait]
pub trait EnforcerGroup {
  /// Get the group id of the user.
  /// User might belong to multiple groups. So return the highest permission group id.
  async fn get_enforce_group_id(&self, uid: &i64) -> Option<String>;
}

pub struct AFEnforcer<T> {
  enforcer: RwLock<Enforcer>,
  pub(crate) metrics_state: MetricsCalState,
  enforce_group: T,
}

impl<T> AFEnforcer<T>
where
  T: EnforcerGroup,
{
  pub async fn new(mut enforcer: Enforcer, enforce_group: T) -> Result<Self, AppError> {
    load_group_policies(&mut enforcer).await?;
    Ok(Self {
      enforcer: RwLock::new(enforcer),
      metrics_state: MetricsCalState::new(),
      enforce_group,
    })
  }

  /// Update policy for a user.
  /// If the policy is already exist, then it will return Ok(false).
  ///
  /// [`ObjectType::Workspace`] has to be paired with [`ActionType::Role`],
  /// [`ObjectType::Collab`] has to be paired with [`ActionType::Level`],
  #[instrument(level = "debug", skip_all, err)]
  pub async fn update_policy(
    &self,
    uid: &i64,
    obj: ObjectType<'_>,
    act: ActionVariant<'_>,
  ) -> Result<(), AppError> {
    validate_obj_action(&obj, &act)?;

    let policies = act
      .policy_acts()
      .into_iter()
      .map(|act| vec![uid.to_string(), obj.policy_object(), act.to_string()])
      .collect::<Vec<Vec<_>>>();

    trace!("[access control]: add policy:{:?}", policies);
    self
      .enforcer
      .write()
      .await
      .add_policies(policies)
      .await
      .map_err(|e| AppError::Internal(anyhow!("fail to add policy: {e:?}")))?;

    Ok(())
  }

  /// Returns policies that match the filter.
  pub async fn remove_policy(
    &self,
    uid: &i64,
    object_type: &ObjectType<'_>,
  ) -> Result<(), AppError> {
    let mut enforcer = self.enforcer.write().await;
    self
      .remove_with_enforcer(uid, object_type, &mut enforcer)
      .await
  }

  /// 1. **Workspace Policy**: Initially, it checks if the user has permission at the workspace level. If the user
  ///    has permission to perform the action on the workspace, the function returns `true` without further checks.
  ///
  /// 2. **Group Policy**: (If applicable) If the workspace policy check fails (`false`), the function will then
  ///    evaluate group-level policies.
  ///
  /// 3. **Object-Specific Policy**: If both previous checks fail, the function finally evaluates the policy
  ///    specific to the object itself.
  ///
  /// ## Parameters:
  /// - `workspace_id`: The identifier for the workspace containing the object.
  /// - `uid`: The user ID of the user attempting the action.
  /// - `obj`: The type of object being accessed, encapsulated within an `ObjectType`.
  /// - `act`: The action being attempted, encapsulated within an `ActionVariant`.
  ///
  /// ## Returns:
  /// - `Ok(true)`: If the user is authorized to perform the action based on any of the evaluated policies.
  /// - `Ok(false)`: If none of the policies authorize the user to perform the action.
  /// - `Err(AppError)`: If an error occurs during policy enforcement.
  ///
  #[instrument(level = "debug", skip_all)]
  pub async fn enforce_policy(
    &self,
    workspace_id: &str,
    uid: &i64,
    obj: ObjectType<'_>,
    act: ActionVariant<'_>,
  ) -> Result<bool, AppError> {
    self
      .metrics_state
      .total_read_enforce_result
      .fetch_add(1, Ordering::Relaxed);

    // 1. First, check workspace-level permissions.
    let workspace_policy_request = WorkspacePolicyRequest::new(workspace_id, uid, &obj, &act);
    let policy = workspace_policy_request.to_policy();
    let mut result = self
      .enforcer
      .read()
      .await
      .enforce(policy)
      .map_err(|e| AppError::Internal(anyhow!("enforce: {e:?}")))?;

    // 2. Fallback to group policy if workspace-level check fails.
    if !result {
      if let Some(guid) = self.enforce_group.get_enforce_group_id(uid).await {
        let policy_request = GroupPolicyRequest::new(&guid, &obj, &act);
        result = self
          .enforcer
          .read()
          .await
          .enforce(policy_request.to_policy())
          .map_err(|e| AppError::Internal(anyhow!("enforce: {e:?}")))?;
      }
    }

    // 3. Finally, enforce object-specific policy if previous checks fail.
    if !result {
      let policy_request = PolicyRequest::new(*uid, &obj, &act);
      let policy = policy_request.to_policy();
      result = self
        .enforcer
        .read()
        .await
        .enforce(policy)
        .map_err(|e| AppError::Internal(anyhow!("enforce: {e:?}")))?;
    }

    Ok(result)
  }

  #[inline]
  async fn remove_with_enforcer(
    &self,
    uid: &i64,
    object_type: &ObjectType<'_>,
    enforcer: &mut Enforcer,
  ) -> Result<(), AppError> {
    let policies_for_user_on_object =
      policies_for_subject_with_given_object(uid, object_type, enforcer).await;

    // if there are no policies for the user on the object, return early.
    if policies_for_user_on_object.is_empty() {
      return Ok(());
    }

    event!(
      tracing::Level::INFO,
      "[access control]: remove policy:user={}, object={}, policies={:?}",
      uid,
      object_type.policy_object(),
      policies_for_user_on_object
    );

    enforcer
      .remove_policies(policies_for_user_on_object)
      .await
      .map_err(|e| AppError::Internal(anyhow!("error enforce: {e:?}")))?;

    Ok(())
  }
}

fn validate_obj_action(obj: &ObjectType<'_>, act: &ActionVariant) -> Result<(), AppError> {
  match (obj, act) {
    (ObjectType::Workspace(_), ActionVariant::FromRole(_))
    | (ObjectType::Collab(_), ActionVariant::FromAccessLevel(_)) => Ok(()),
    _ => Err(AppError::Internal(anyhow!(
      "invalid object type and action type combination: object={:?}, action={:?}",
      obj,
      act.to_enforce_act()
    ))),
  }
}
#[inline]
async fn policies_for_subject_with_given_object<T: ToString>(
  subject: T,
  object_type: &ObjectType<'_>,
  enforcer: &Enforcer,
) -> Vec<Vec<String>> {
  let subject = subject.to_string();
  let object_type_id = object_type.policy_object();
  let policies_related_to_object =
    enforcer.get_filtered_policy(POLICY_FIELD_INDEX_OBJECT, vec![object_type_id]);

  policies_related_to_object
    .into_iter()
    .filter(|p| p[POLICY_FIELD_INDEX_SUBJECT] == subject)
    .collect::<Vec<_>>()
}

pub struct NoEnforceGroup;
#[async_trait]
impl EnforcerGroup for NoEnforceGroup {
  async fn get_enforce_group_id(&self, _uid: &i64) -> Option<String> {
    None
  }
}
