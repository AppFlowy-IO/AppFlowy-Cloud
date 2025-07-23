use crate::casbin::access::{POLICY_FIELD_INDEX_OBJECT, POLICY_FIELD_INDEX_SUBJECT};
use crate::entity::{ObjectType, SubjectType};
use casbin::{CachedEnforcer, MgmtApi};

#[inline]
pub(crate) async fn policies_for_subject_with_given_object(
  subject: SubjectType,
  object_type: ObjectType,
  enforcer: &CachedEnforcer,
) -> Vec<Vec<String>> {
  let subject_id = subject.policy_subject();
  let object_type_id = object_type.policy_object();
  let policies_related_to_object =
    enforcer.get_filtered_policy(POLICY_FIELD_INDEX_OBJECT, vec![object_type_id]);

  policies_related_to_object
    .into_iter()
    .filter(|p| p[POLICY_FIELD_INDEX_SUBJECT] == subject_id)
    .collect::<Vec<_>>()
}

#[cfg(test)]
pub mod tests {
  use crate::casbin::access::{casbin_model, cmp_role_or_level};
  use crate::casbin::enforcer_v2::AFEnforcerV2;
  use casbin::function_map::OperatorFunction;
  use casbin::{CoreApi, MemoryAdapter};
  pub async fn test_enforcer_v2() -> AFEnforcerV2 {
    let model = casbin_model().await.unwrap();
    let mut enforcer = casbin::CachedEnforcer::new(model, MemoryAdapter::default())
      .await
      .unwrap();

    enforcer.add_function("cmpRoleOrLevel", OperatorFunction::Arg2(cmp_role_or_level));
    AFEnforcerV2::new(enforcer).await.unwrap()
  }
}
