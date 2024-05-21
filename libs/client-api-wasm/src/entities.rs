use client_api::entity::AFUserProfile;
use client_api::error::{AppResponseError, ErrorCode};
use collab_entity::{CollabType, EncodedCollab};
use database_entity::dto::{
  AFUserWorkspaceInfo, AFWorkspace, BatchQueryCollabResult, QueryCollab, QueryCollabParams,
  QueryCollabResult,
};
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::collections::HashMap;
use tsify::Tsify;
use wasm_bindgen::JsValue;

macro_rules! from_struct_for_jsvalue {
  ($type:ty) => {
    impl From<$type> for JsValue {
      fn from(value: $type) -> Self {
        match serde_wasm_bindgen::to_value(&value) {
          Ok(js_value) => js_value,
          Err(err) => {
            tracing::error!("Failed to convert User to JsValue: {:?}", err);
            JsValue::NULL
          },
        }
      }
    }
  };
}

#[derive(Tsify, Serialize, Deserialize, Default, Debug)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct Configuration {
  pub compression_quality: u32,
  pub compression_buffer_size: usize,
}

#[derive(Tsify, Serialize, Deserialize, Default, Debug)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct ClientAPIConfig {
  pub base_url: String,
  pub ws_addr: String,
  pub gotrue_url: String,
  pub device_id: String,
  pub configuration: Option<Configuration>,
  pub client_id: String,
}

#[derive(Tsify, Serialize, Deserialize, Default, Debug)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct ClientResponse {
  pub code: ErrorCode,
  pub message: String,
}

from_struct_for_jsvalue!(ClientResponse);
impl From<AppResponseError> for ClientResponse {
  fn from(err: AppResponseError) -> Self {
    ClientResponse {
      code: err.code,
      message: err.message.to_string(),
    }
  }
}

#[derive(Tsify, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct User {
  pub uid: String,
  pub uuid: String,
  pub email: Option<String>,
  pub name: Option<String>,
  pub latest_workspace_id: String,
  pub icon_url: Option<String>,
}

from_struct_for_jsvalue!(User);
impl From<AFUserProfile> for User {
  fn from(profile: AFUserProfile) -> Self {
    User {
      uid: profile.uid.to_string(),
      uuid: profile.uuid.to_string(),
      email: profile.email,
      name: profile.name,
      latest_workspace_id: profile.latest_workspace_id.to_string(),
      icon_url: None,
    }
  }
}

#[derive(Tsify, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct UserWorkspace {
  pub user: User,
  pub visiting_workspace_id: String,
  pub workspaces: Vec<Workspace>,
}

from_struct_for_jsvalue!(UserWorkspace);

impl From<AFUserWorkspaceInfo> for UserWorkspace {
  fn from(info: AFUserWorkspaceInfo) -> Self {
    UserWorkspace {
      user: User::from(info.user_profile),
      visiting_workspace_id: info.visiting_workspace.workspace_id.to_string(),
      workspaces: info.workspaces.into_iter().map(Workspace::from).collect(),
    }
  }
}

#[derive(Tsify, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct Workspace {
  pub workspace_id: String,
  pub database_storage_id: String,
  pub owner_uid: String,
  pub owner_name: String,
  pub workspace_type: i32,
  pub workspace_name: String,
  pub created_at: String,
  pub icon: String,
}

from_struct_for_jsvalue!(Workspace);

impl From<AFWorkspace> for Workspace {
  fn from(workspace: AFWorkspace) -> Self {
    Workspace {
      workspace_id: workspace.workspace_id.to_string(),
      database_storage_id: workspace.database_storage_id.to_string(),
      owner_uid: workspace.owner_uid.to_string(),
      owner_name: workspace.owner_name,
      workspace_type: workspace.workspace_type,
      workspace_name: workspace.workspace_name,
      created_at: workspace.created_at.timestamp().to_string(),
      icon: workspace.icon,
    }
  }
}

#[derive(Tsify, Serialize, Deserialize, Default, Debug)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct ClientQueryCollabParams {
  pub workspace_id: String,
  pub object_id: String,
  #[tsify(type = "0 | 1 | 2 | 3 | 4 | 5")]
  pub collab_type: i32,
}

impl From<ClientQueryCollabParams> for QueryCollabParams {
  fn from(value: ClientQueryCollabParams) -> QueryCollabParams {
    QueryCollabParams {
      workspace_id: value.workspace_id,
      inner: QueryCollab {
        collab_type: CollabType::from(value.collab_type),
        object_id: value.object_id,
      },
    }
  }
}

#[derive(Tsify, Serialize, Deserialize, Default)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct ClientEncodeCollab {
  pub state_vector: Vec<u8>,
  pub doc_state: Vec<u8>,
  #[serde(default)]
  pub version: ClientEncoderVersion,
}

#[derive(Tsify, Default, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum ClientEncoderVersion {
  #[default]
  V1 = 0,
  V2 = 1,
}

from_struct_for_jsvalue!(ClientEncodeCollab);

impl From<EncodedCollab> for ClientEncodeCollab {
  fn from(collab: EncodedCollab) -> Self {
    ClientEncodeCollab {
      state_vector: collab.state_vector.to_vec(),
      doc_state: collab.doc_state.to_vec(),
      version: ClientEncoderVersion::V1,
    }
  }
}

#[derive(Tsify, Serialize, Deserialize, Default, Debug)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct BatchClientQueryCollab(pub Vec<ClientQueryCollab>);
#[derive(Tsify, Serialize, Deserialize, Default, Debug)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct ClientQueryCollab {
  pub object_id: String,
  #[tsify(type = "0 | 1 | 2 | 3 | 4 | 5")]
  pub collab_type: i32,
}

from_struct_for_jsvalue!(ClientQueryCollab);

impl From<ClientQueryCollab> for QueryCollab {
  fn from(value: ClientQueryCollab) -> QueryCollab {
    QueryCollab {
      collab_type: CollabType::from(value.collab_type),
      object_id: value.object_id,
    }
  }
}

#[derive(Tsify, Serialize, Deserialize, Default)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct BatchClientEncodeCollab(pub HashMap<String, ClientEncodeCollab>);

from_struct_for_jsvalue!(BatchClientEncodeCollab);

impl From<BatchQueryCollabResult> for BatchClientEncodeCollab {
  fn from(result: BatchQueryCollabResult) -> Self {
    let mut hash_map = HashMap::new();

    result.0.into_iter().for_each(|(k, v)| match v {
      QueryCollabResult::Success { encode_collab_v1 } => {
        EncodedCollab::decode_from_bytes(&encode_collab_v1)
          .map(|collab| {
            hash_map.insert(k, ClientEncodeCollab::from(collab));
          })
          .unwrap_or_else(|err| {
            tracing::error!("Failed to decode collab: {:?}", err);
          });
      },
      QueryCollabResult::Failed { .. } => {
        tracing::error!("Failed to get collab: {:?}", k);
      },
    });

    BatchClientEncodeCollab(hash_map)
  }
}
