/** @type {import('@maizzle/framework').Config} */

/*
|-------------------------------------------------------------------------------
| Development config                      https://maizzle.com/docs/environments
|-------------------------------------------------------------------------------
|
| The exported object contains the default Maizzle settings for development.
| This is used when you run `maizzle build` or `maizzle serve` and it has
| the fastest build time, since most transformations are disabled.
|
*/

module.exports = {
  build: {
    templates: {
      source: "src/templates",
      destination: {
        path: "build_local",
      },
      assets: {
        source: "src/images",
        destination: "images",
      },
    },
  },
  locals: {
    cdnBaseUrl: "",
    userIconUrl: "https://cdn-icons-png.flaticon.com/512/1077/1077012.png",
    error: "Test error message",
    detailError: "Test detail error message",
    userName: "John Doe",
    acceptUrl: "https://appflowy.io",
    approveUrl: "https://appflowy.io",
    launchWorkspaceUrl: "https://appflowy.io",
    workspaceName: "AppFlowy",
    workspaceMembersCount: "100",
    workspaceIconURL: "https://cdn-icons-png.flaticon.com/512/1078/1078013.png",
    mentionedPageName: "Test Page",
    mentionedPageUrl: "https://appflowy.io",
    mentionerName: "John Doe",
    mentionerIconUrl: "https://cdn-icons-png.flaticon.com/512/1077/1077012.png",
    mentionedAt: "Jul 22, 2025, 3:42 PM (UTC)",
  },
};
