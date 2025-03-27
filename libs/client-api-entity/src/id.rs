use uuid::Uuid;

pub fn user_awareness_object_id(user_uuid: &Uuid, workspace_id: &Uuid) -> Uuid {
  Uuid::new_v5(
    user_uuid,
    format!("user_awareness:{}", workspace_id).as_bytes(),
  )
}
