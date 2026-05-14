# Authentication

Follow [this](https://appflowy.com/docs/Authentication) guide to set up

## Authentik OAuth2/OIDC (patched GoTrue image)

This repository includes a GoTrue patch (applied in `docker/gotrue/Dockerfile`) that adds an `authentik` OAuth provider.

Set these environment variables:

```env
GOTRUE_EXTERNAL_AUTHENTIK_ENABLED=true
GOTRUE_EXTERNAL_AUTHENTIK_CLIENT_ID=<client-id>
GOTRUE_EXTERNAL_AUTHENTIK_SECRET=<client-secret>
GOTRUE_EXTERNAL_AUTHENTIK_REDIRECT_URI=${API_EXTERNAL_URL}/callback
GOTRUE_EXTERNAL_AUTHENTIK_URL=https://auth.example.com
```

`GOTRUE_EXTERNAL_AUTHENTIK_URL` must be the Authentik base URL (without trailing slash).

For an OAuth-only setup, keep SAML disabled:

```env
GOTRUE_SAML_ENABLED=false
```

For a mixed OAuth + SSO/SAML setup, enable SAML and create SSO providers from the admin UI:

```env
GOTRUE_SAML_ENABLED=true
```
