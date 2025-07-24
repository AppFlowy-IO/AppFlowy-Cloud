/** @type {import('@maizzle/framework').Config} */

/*
|-------------------------------------------------------------------------------
| Production config                       https://maizzle.com/docs/environments
|-------------------------------------------------------------------------------
|
| This is where you define settings that optimize your emails for production.
| These will be merged on top of the base config.js, so you only need to
| specify the options that are changing.
|
*/

module.exports = {
  build: {
    templates: {
      destination: {
        path: "../assets/mailer_templates/build_production",
      },
    },
  },
  locals: {
    cdnBaseUrl:
      "https://raw.githubusercontent.com/AppFlowy-IO/AppFlowy-Cloud/main/assets/mailer_templates/build_production/",
    error: "{{ error }}",
    detailError: "{{ error_detail }}",
    userIconUrl: "{{ user_icon_url }}",
    importFileName: "{{ import_file_name }}",
    importTaskId: "{{ import_task_id }}",
    userName: "{{ username }}",
    acceptUrl: "{{ accept_url }}",
    approveUrl: "{{ approve_url }}",
    launchWorkspaceUrl: "{{ launch_workspace_url }}",
    workspaceName: "{{ workspace_name }}",
    workspaceMembersCount: "{{ workspace_member_count }}",
    workspaceIconURL: "{{ workspace_icon_url }}",
    mentionedPageName: "{{ mentioned_page_name }}",
    mentionedPageUrl: "{{ mentioned_page_url }}",
    mentionerName: "{{ mentioner_name }}",
    mentionerIconUrl: "{{ mentioner_icon_url }}",
    mentionedAt: "{{ mentioned_at }}",
  },
  inlineCSS: true,
  removeUnusedCSS: true,
  shorthandCSS: true,
  prettify: true,
};
