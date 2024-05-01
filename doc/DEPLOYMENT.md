# Deployment

- AppFlowy-Cloud is designed to be easily self deployed for self managed cloud storage
- The following document will walk you through the steps to deploy your own AppFlowy-Cloud

## Hardware Requirements

- Minimum 2GB Ram (4GB Recommended)
- Ports 80/443 available
- Because AppFlowy-Cloud will have to be running persistently (or at least whenever users need access),
  it's probably a good idea to run it on a non-end user device. It is best if you already have a home server (check
  software requirements),
  but if you don't, you can also deploy it on a cloud compute service as host server such as
    - [Amazon EC2](https://aws.amazon.com/ec2/) or
    - [Azure Virtual Machines](https://azure.microsoft.com/en-gb/products/virtual-machines/)

## Software Requirements

Ensure you have Docker Compose(v2) installed on your host server. Follow the official guidelines for a reliable setup:

Docker Compose is included with Docker Engine:

- **Docker Engine:** We suggest adhering to the instructions provided by Docker
  for [installing Docker Engine](https://docs.docker.com/engine/install/).

For older versions of Docker Engine that do not include Docker Compose:

- **Docker Compose:** Install it as per the [official documentation](https://docs.docker.com/compose/install/).

Once you have it installed, you can check by running the command:

```
docker compose version
# Docker Compose version 2.23.3
```

Note: `docker-compose` (with the hyphen) may not be supported. You are advised to use `docker compose` (without hyphen)
instead.

## Steps

> **🚀🚀🚀 Quick Try: Step-by-Step Guide**
>
> For an in-depth, step-by-step guide on self-hosting AppFlowy Cloud on AWS EC2, particularly for demonstration
> purposes, please consult our detailed documentation:
> [Self-Hosting AppFlowy Cloud on AWS EC2 - Step by Step Guide](./EC2_SELF_HOST_GUIDE.md)
>
> **Note:** This guide is tailored for demonstration purposes using the AWS EC2 free tier. For more customized
> deployment options, please follow the subsequent steps outlined below.

### 1. Getting source files

- Clone this repository into your host server and `cd` into it

```bash
git clone https://github.com/AppFlowy-IO/AppFlowy-Cloud
cd AppFlowy-Cloud
```

### 2. Preparing the configuration

- This is perhaps the most important part of the deployment process, please read carefully.
- It is required that there is a `.env` file in the root directory of the repository.
- To get started, copy the template `deploy.env` as `.env` using the following shell commands:

```bash
cp deploy.env .env
```

- Kindly read through the comments for each option
- Modify the values in `.env` according to your needs

For authentication details, refer to the [Authentication](./AUTHENTICATION.md) documentation. You will need to update
the
redirect URI to match your host server's public IP or hostname, such
as `http://<your-host-server-public-ip-or-hostname>/gotrue/callback`.
If using localhost, then just keep the default value.

```bash
GOTRUE_EXTERNAL_GOOGLE_ENABLED=true
GOTRUE_EXTERNAL_GOOGLE_CLIENT_ID=
GOTRUE_EXTERNAL_GOOGLE_SECRET=
GOTRUE_EXTERNAL_GOOGLE_REDIRECT_URI=http://your-host/gotrue/callback

# GitHub OAuth2
GOTRUE_EXTERNAL_GITHUB_ENABLED=true
GOTRUE_EXTERNAL_GITHUB_CLIENT_ID=your-github-client-id
GOTRUE_EXTERNAL_GITHUB_SECRET=your-github-secret
GOTRUE_EXTERNAL_GITHUB_REDIRECT_URI=http://your-host/gotrue/callback

# Discord OAuth2
GOTRUE_EXTERNAL_DISCORD_ENABLED=true
GOTRUE_EXTERNAL_DISCORD_CLIENT_ID=your-discord-client-id
GOTRUE_EXTERNAL_DISCORD_SECRET=your-discord-secret
GOTRUE_EXTERNAL_DISCORD_REDIRECT_URI=http://your-host/gotrue/callback
```

### 3. Running the services

#### Start and run AppFlowy-Cloud

- The following command will build and start AppFlowy-Cloud.

```bash
docker compose up -d
```

- Please check that all the services are running

```bash
docker ps -a
```

### 4. Optional Services

We have provided optional services in the file `docker-compose-extra.yml`.
You do not need them for a fully functional installation of AppFlowy Cloud, but they could be helpful for various
admin/debug tasks.

- `pgadmin` (Web UI to visualize the provided postgres database)
- `portainer`/`portainer_init` (Web UI to provide some monitoring and ease of container management)
- `tunnel` (Cloudflare tunnel to provide a secure way to connect AppFlowy to Cloudflare without a publicly routable IP
  address)
- `admin_frontend` (admin portal to manage accounts and add authentication methods. We recommend to keep this)
  If you wish to deploy those, edit the file accordingly and do:

```
docker compose --file docker-compose-extras.yml up -d
```

You may ignore the orphan containers warning message that docker will output.


> When using the `docker compose up -d` command without specifying a tag, Docker Compose will pull the `latest`
> tag for the `appflowy_cloud` and `admin_frontend` images from Docker Hub by default. If you've set
> the `APPFLOWY_CLOUD_VERSION`
> or the `APPFLOWY_ADMIN_FRONTEND_VERSION` environment variable, it will pull the specified version instead.

- Check that services are running correctly `docker ps -a`
- If you find a particular service not working properly, you can inspect the logs:

```bash
# Getting logs for a particular docker compose service
# You can obtain its name by running `docker ps -a`
docker logs <NAME>
# e.g. docker logs appflowy-cloud-admin_frontend-1
```

### 5. Reconfiguring and redeployment

- It is very common to reconfigure and restart. To do so, simply edit the `.env` and run `docker compose up -d` again

## Ports

- After Deployment, you should see that AppFlowy-Cloud is serving 2 ports
- `443` (https)
- `80`  (http)
- Your host server need to expose either of these ports.

## SSL Certificate

- To use your own SSL certificates for https, replace `certificate.crt` and `private_key.key`
  with your own in `nginx/ssl/` directory.

## Usage of AppFlowy Application with AppFlowy Cloud

- [AppFlowy with AppFlowyCloud](https://docs.appflowy.io/docs/guides/appflowy/self-hosting-appflowy)

## 5. FAQ

- How do I use a different `postgres`?
  The default url is using the postgres in docker compose, in service `appflowy_cloud` and `gotrue` respectively.
  However it is possible to change the database storage for it. The following steps are listed below.

1. You need set `APPFLOWY_DATABASE_URL` to another postgres url.

```
APPFLOWY_DATABASE_URL=postgres://<postgres_user>:<password>@<host>:<port>/<dbname>
```

2. You also need to set `GOTRUE_DATABASE_URL` to use the same postgres database.

```
GOTRUE_DATABASE_URL=postgres://supabase_auth_admin:root@<host>:<port>/<dbname>
```

- `supabase_auth_admin` and `root` must be kept in sync with the init migration scripts from `migrations/before`.
  Currently it's possible to change the password, but probably can't change the username.
- `dbname` for `appflowy_cloud` and `gotrue` must be the same.

3. You would need to run the initialization sql file from `migrations/before` in your hosted postgres.
