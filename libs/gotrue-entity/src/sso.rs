use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SSOProviders {
  pub items: Option<Vec<SSOProvider>>,
}

#[derive(Debug, Deserialize)]
pub struct SSOProvider {
  pub id: String,
  pub saml: SAMLProvider,
  pub domains: Vec<String>,
  pub created_at: String,
  pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct SAMLProvider {
  pub entity_id: String,
  pub metadata_xml: Option<String>,
  pub metadata_url: Option<String>,
  pub attribute_mapping: SAMLAttributeMapping,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SAMLAttributeMapping {
  pub keys: Option<BTreeMap<String, SAMLAttribute>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SAMLAttribute {
  pub name: Option<String>,
  pub names: Option<Vec<String>>,
  pub default: serde_json::Value,
}

pub struct SSODomain {
  pub domain: String,
}
