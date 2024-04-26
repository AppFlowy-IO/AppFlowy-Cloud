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
        path: '../assets/mailer_templates/build_production',
      },
    },
  },
  locals: {
    cdnBaseUrl: 'https://raw.githubusercontent.com/AppFlowy-IO/AppFlowy-Cloud/main/assets/mailer_templates/build_production/',
    userIconUrl: "{{ .UserIconURL }}",
    userName: "{{ .UserName }}",
    acceptUrl: "{{ .AcceptUrl }}",
    workspaceName: "{{ .WorkspaceName }}",
    workspaceMembersCount: "{{ .WorkspaceMembersCount }}",
    workspaceIconURL: "{{ .WorkspaceIconURL }}",
  },
  inlineCSS: true,
  removeUnusedCSS: true,
  shorthandCSS: true,
  prettify: true,
}
