# Optional SCIM endpoint

AppFlowy Self-hosted Cloud can act as a SCIM 2.0 server. Your identity provider
pushes Users and Groups to AppFlowy; SCIM is provisioning, not login.

The public SCIM route is disabled by default. A normal deployment remains:

```bash
docker compose up -d
```

## Enable SCIM

SCIM bearer tokens grant permission to provision and deactivate accounts, so
the endpoint is available only over HTTPS. Configure `SCHEME=https`, install a
valid certificate in `nginx/ssl`, then start the stack with the SCIM override:

```bash
docker compose \
  -f docker-compose.yml \
  -f docker-compose.scim.yml \
  up -d
```

The override adds no services and does not deploy an identity provider. It only
mounts the `/scim/` reverse-proxy route into the existing Nginx service, so it
works with any compatible provider, including Microsoft Entra ID, Okta, and
Authentik.

In the AppFlowy Admin Console:

1. Open **SCIM Provisioning**.
2. Create a directory-sync connection for the target workspace.
3. Copy the returned bearer token immediately and store it securely.
4. Configure the identity provider with:
   - **Tenant URL:** `https://<your-domain>/scim/v2`
   - **Token:** the connection's bearer token

Each directory-sync connection targets one AppFlowy workspace. SCIM Groups are
directory groups inside that connection; they are not separate workspaces.

Plain HTTP requests receive `403 Forbidden` rather than a redirect. The
SCIM-enabled virtual server also omits query strings from access logs and
retains only critical Nginx error messages because SCIM filters can contain
email addresses and external IDs.

## External reverse proxies

The override assumes TLS terminates at the bundled Nginx container. If a load
balancer or external proxy terminates TLS and forwards plaintext HTTP to the
container, configure `/scim/` on that HTTPS edge instead of using this override:

- forward the original `/scim/v2/*` URI to `appflowy_cloud:8000`;
- preserve the `Authorization` header;
- do not redirect plaintext SCIM requests to HTTPS;
- omit query strings from access and error logs.

The `appflowy_cloud` hostname is available only inside the Compose network. An
external proxy must either join that network or use another private, reachable
address for the AppFlowy Cloud service; port 8000 is not published by default.

## Disable SCIM

Remove the override from subsequent Compose commands and recreate Nginx:

```bash
docker compose -f docker-compose.yml up -d --force-recreate nginx
```

This removes the public `/scim/` route. Revoke the directory-sync connection in
the Admin Console as well if the provider must no longer provision users.
