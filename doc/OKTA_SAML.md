# Okta Authentication via SAML
- AppFlowy supports Identity Provider(Idp) that uses SAML Assertion
- One example of such Idp is [Okta](https://www.okta.com)
- After the setup, you will be able to launch AppFlowy from Okta
- Feel free to reach us on Discord or create a GitHub issue if you have any problems related to the integration

## Getting started
- This guide assumes the following
  - You are an Admin of Okta Identity Provider
  - You have AppFlowy-Cloud deployed [Deployment](./DEPLOYMENT.md)

## Steps (Okta)
![Click On Admin](../assets/images/okta_integration/click_on_admin.png)
- Click "Admin" on the top right corner of Okta dashboard/home page

![Click On Applications](../assets/images/okta_integration/click_on_applications.png)
- Click the top left menu bar, then under "Applications", click "Applications"

![Create App Integration](../assets/images/okta_integration/create_app_integration.png)
- Click "Create App Integration"

![Choose SAML then next](../assets/images/okta_integration/choose_saml_then_next.png)
- Select SAML 2.0 then click "Next"

![Okta create App](../assets/images/okta_integration/app_general_settings.png)
- In general settings, use "AppFlowy" as "App name"
- Optional: Select a logo
- Click "Next"

![Configure SAML Integration](../assets/images/okta_integration/configure_saml.png)
In General
- Use `https://<your_host>/gotrue/sso/saml/acs` for "Single sign-on URL"
- Use `https://<your_host>/gotrue/sso/saml/metadata` for "Audience URI (SP Entity ID)"
In Attribute Statements (optional)
- Use `email` for "Name"
- Select "user.email" in the drop down for "Value"
- Click "Next"

![SAML Integration Feedback](../assets/images/okta_integration/saml_integration_feedback.png)
- Use `https://<your_host>/gotrue/sso/saml/acs` for "Single sign-on URL"
- Use `https://<your_host>/gotrue/sso/saml/metadata` for "Audience URI (SP Entity ID)"
In Attribute Statements (optional)
- Select "I'm an Okta customer adding an internal app"
- Tick "This is an internal app that we have created"
- Click "Finish"

## Steps (AppFlowy)
![AppFlowy Click Admin](../assets/images/okta_integration/appflowy_click_admin.png)
- Login as Admin in `https://<your_host>/web/login`
- Click "Admin" on the top right corner

![AppFlowy Click Admin](../assets/images/okta_integration/appflowy_click_admin.png)
- Login as Admin in `https://<your_host>/web/login`
- Click "Admin" on the top right corner

![Copy Metadata URL](../assets/images/okta_integration/copy_metadata_url.png)
- Go back to okta, navigate to "Applications" -> "AppFlowy" -> "Sign On", then copy the Metadata URL

![AppFlowy Create SSO](../assets/images/okta_integration/appflowy_create_sso.png)
- In AppFlowy Admin page, Click on "Create SSO" on the left, paste the Metadata URL, then click "Create"

![Check SSO](../assets/images/okta_integration/appflowy_list_sso.png)
- In AppFlowy Admin page, Click on "List SSO", you should see the SSO being created

## App Visibility
In order for AppFlowy to be available for users, you may need to do the following
![Assign AppFlowy](../assets/images/okta_integration/assign_appflowy.png)
- In okta Admin -> "Applications" -> "AppFlowy", click on the settings icon
- Assign to various user or groups as needed by your organisation

![Open AppFlowy](../assets/images/okta_integration/open_appflowy.png)
- In okta user page, you should see "AppFlowy" added
- Clicking on it should launch the App
