use askama::Template;
use database_entity::dto::{AFWorkspace, AFWorkspaceInvitation};
use gotrue_entity::{dto::User, sso::SSOProvider};

use crate::{askama_entities::WorkspaceWithMembers, ext::entities::WorkspaceUsageLimits};

#[derive(Template)]
#[template(path = "pages/redirect.html")]
pub struct Redirect {
  pub redirect_url: String,
}

#[derive(Template)]
#[template(path = "pages/open_appflowy_or_download.html")]
pub struct OpenAppFlowyOrDownload {}

#[derive(Template)]
#[template(path = "pages/login_callback.html")]
pub struct LoginCallback {}

#[derive(Template)]
#[template(path = "components/user_usage.html")]
pub struct UserUsage {
  pub workspace_count: usize,
  pub workspace_limit: String,
}

// ./../templates/components/workspace_usage.html
#[derive(Template)]
#[template(path = "components/workspace_usage.html")]
pub struct WorkspaceUsageList {
  pub workspace_usages: Vec<WorkspaceUsageLimits>,
}

#[derive(Template)]
#[template(path = "components/admin_sso_detail.html")]
pub struct SsoDetail {
  pub sso_provider: SSOProvider,
  pub mapping_json: String,
}

#[derive(Template)]
#[template(path = "components/admin_sso_create.html")]
pub struct SsoCreate;

#[derive(Template)]
#[template(path = "components/admin_sso_list.html")]
pub struct SsoList {
  pub sso_providers: Vec<SSOProvider>,
}

#[derive(Template)]
#[template(path = "components/change_password.html")]
pub struct ChangePassword;

#[derive(Template)]
#[template(path = "pages/login.html")]
pub struct Login<'a> {
  pub oauth_providers: Vec<&'a str>,
}

#[derive(Template)]
#[template(path = "pages/home.html")]
pub struct Home<'a> {
  pub user: &'a User,
  pub is_admin: bool,
}

#[derive(Template)]
#[template(path = "components/create_user.html")]
pub struct CreateUser;

#[derive(Template)]
#[template(path = "components/invite.html")]
pub struct Invite {
  pub shared_workspaces: Vec<AFWorkspace>,
  pub owned_workspaces: Vec<WorkspaceWithMembers>,
  pub pending_workspace_invitations: Vec<AFWorkspaceInvitation>,
}

#[derive(Template)]
#[template(path = "components/shared_workspaces.html")]
pub struct SharedWorkspaces {
  pub shared_workspaces: Vec<AFWorkspace>,
}

#[derive(Template)]
#[template(path = "components/admin_navigate.html")]
pub struct AdminNavigate;

#[derive(Template)]
#[template(path = "components/navigate.html")]
pub struct Navigate;

#[derive(Template)]
#[template(path = "pages/admin_home.html")]
pub struct AdminHome<'a> {
  pub user: &'a User,
}

#[derive(Template)]
#[template(path = "components/admin_users.html")]
pub struct AdminUsers<'a> {
  pub users: &'a [gotrue_entity::dto::User],
}

#[derive(Template)]
#[template(path = "components/user_details.html")]
pub struct UserDetails<'a> {
  pub user: &'a gotrue_entity::dto::User,
}

#[derive(Template)]
#[template(path = "components/admin_user_details.html")]
pub struct AdminUserDetails<'a> {
  pub user: &'a gotrue_entity::dto::User,
}

// Any filter defined in the module `filters` is accessible in your template.
mod filters {
  pub fn default<T: std::fmt::Display>(
    input: &Option<T>,
    default_val: &str,
  ) -> ::askama::Result<String> {
    Ok(
      input
        .as_ref()
        .map(|i| i.to_string())
        .unwrap_or_else(|| default_val.to_string()),
    )
  }
}
