use async_trait::async_trait;
use collab_database::database::timestamp;
use collab_folder::ViewLayout;
use serde_json::Value;

use crate::document::util::create_document_from_json;
use crate::hierarchy_builder::{ViewBuilder, WorkspaceViewBuilder};
use crate::{gen_view_id, TemplateData, WorkspaceTemplate};

pub struct VaultTemplate;

impl VaultTemplate {
  async fn create_generate(&self, general_view_uuid: String) -> anyhow::Result<TemplateData> {
    let default_space_json = include_str!("../../assets/default_space.json");
    let general_data =
      create_document_from_json(general_view_uuid.clone(), default_space_json).await?;

    Ok(general_data)
  }

  async fn create_vault_get_started(&self, object_id: String) -> anyhow::Result<TemplateData> {
    let getting_started_json = include_str!("../../assets/vault_get_started.json");
    let getting_started_json: Value = serde_json::from_str(getting_started_json)?;
    let getting_started_data =
      create_document_from_json(object_id.clone(), &getting_started_json.to_string()).await?;

    Ok(getting_started_data)
  }

  async fn build_getting_started_view(
    &self,
    view_builder: ViewBuilder,
    getting_started_view_uuid: String,
  ) -> ViewBuilder {
    // getting started view
    view_builder
            .with_name("Vault Mode")
            .with_icon("🏦")
            .with_extra(r#"{"font_layout":"normal","line_height_layout":"normal","cover":{"type":"gradient","value":"appflowy_them_color_gradient4"},"font":null}"#)
            .with_view_id(getting_started_view_uuid)
  }
}

#[async_trait]
impl WorkspaceTemplate for VaultTemplate {
  fn layout(&self) -> ViewLayout {
    ViewLayout::Document
  }

  async fn create(&self, _object_id: String) -> anyhow::Result<Vec<TemplateData>> {
    unreachable!("This function is not supposed to be called.")
  }

  async fn create_workspace_view(
    &self,
    _uid: i64,
    workspace_view_builder: &mut WorkspaceViewBuilder,
  ) -> anyhow::Result<Vec<TemplateData>> {
    let general_view_uuid = gen_view_id().to_string();
    let getting_started_uuid = gen_view_id().to_string();

    let general_data = self.create_generate(general_view_uuid.clone()).await?;
    let get_started_data = self
      .create_vault_get_started(getting_started_uuid.clone())
      .await?;
    workspace_view_builder
      .with_view_builder(|view_builder| async {
        let created_at = timestamp();
        let mut view_builder = view_builder
         .with_view_id(general_view_uuid.clone())
         .with_name("General")
         .with_extra(&format!(
             "{{\"is_space\":true,\"space_icon\":\"interface_essential/home-3\",\"space_icon_color\":\"0xFFA34AFD\",\"space_permission\":1,\"space_created_at\":{}}}",
             created_at
         ));

          let cloned_getting_started_uuid = getting_started_uuid.clone();
         view_builder = view_builder.with_child_view_builder(|child_view_builder| async {
             let child_view_builder = self.build_getting_started_view(child_view_builder, cloned_getting_started_uuid.clone()).await;
             child_view_builder.build()
         }).await;

        view_builder.build()
      })
      .await;

    let template_data = vec![general_data, get_started_data];
    Ok(template_data)
  }
}
