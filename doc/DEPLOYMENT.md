# Deployment
- AppFlowy-Cloud is designed to be easily self deployed for self managed cloud storage
- The following document will walk you through the steps to deploy your own AppFlowy-Cloud

## Hardware Requirements
- Minimum 2GB Ram (4GB Recommended)
- Ports 80/443 available
- Because AppFlowy-Cloud will have to be running persistently (or at least whenever users need access),
it's probably a good idea to run it on a non-end user device. It is best if you already have a home server(check software requirements),
but if you don't, you can also deploy it on a cloud compute services as host server such as
    - [Amazon EC2](https://aws.amazon.com/ec2/) or
    - [Azure Virtual Machines](https://azure.microsoft.com/en-gb/products/virtual-machines/)

## Software Requirements
Ensure you have Docker Compose(v2) installed on your host server. Follow the official guidelines for a reliable setup:

Docker Compose is included with Docker Engine:
- **Docker Engine:** We suggest adhering to the instructions provided by Docker for [installing Docker Engine](https://docs.docker.com/engine/install/).

For older versions of Docker Engine that do not include Docker Compose:
- **Docker Compose:** Install it as per the [official documentation](https://docs.docker.com/compose/install/).

Once you have it installed, you can check by running the command:
```
docker compose version
# Docker Compose version 2.23.3
```
Note: `docker-compose` (with the hyphen) may not be supported. You are advise to use `docker compose`(without hyphen) instead.

## Steps

> **🚀🚀🚀 Quick Try: Step-by-Step Guide**
>
> For an in-depth, step-by-step guide on self-hosting AppFlowy Cloud on AWS EC2, particularly for demonstration purposes, please consult our detailed documentation:
> [Self-Hosting AppFlowy Cloud on AWS EC2 - Step by Step Guide](./EC2_SELF_HOST_GUIDE.md)
>
> **Note:** This guide is tailored for demonstration purposes using the AWS EC2 free tier. For more customized deployment options, please follow the subsequent steps outlined below.


### 1. Getting source files
- Clone this repository into your host server and `cd` into it
```bash
git clone https://github.com/AppFlowy-IO/AppFlowy-Cloud
cd AppFlowy-Cloud
```

### 2. Preparing the configuration
- This is perhaps the most important part of the deployment process, please read carefully.
- It is required that that is a `.env` file in the root directory of the repository.
- To get started, copy the template `deploy.env` as `.env` using the following shell commands:
```bash
cp deploy.env .env
```
- There will be values in the `.env` that needs to be change according to your needs
- Kindly read the following comments for each set of settings
```bash
# This is the secret key for authentication, please change this and keep the key safe
GOTRUE_JWT_SECRET=hello456

# This determine if the user will be user automatically be confirmed(verified) when they sign up
# If this is enabled, it requires a clicking a confirmation link in the email after a user signs up.
# If you do not have SMTP service set up, or any other OAuth2 method, you should set this to true,
# or else no user will be able to be authenticated
GOTRUE_MAILER_AUTOCONFIRM=true

# If you require mail confirmation, you need to set the SMTP configuration below
# and set `GOTRUE_MAILER_AUTOCONFIRM` to be false
GOTRUE_SMTP_HOST=smtp.gmail.com
GOTRUE_SMTP_PORT=465
GOTRUE_SMTP_USER=user1@example.com
# this is typically an app password that you would need to generate: https://myaccount.google.com/apppasswords
GOTRUE_SMTP_PASS=somesecretkey
# You can leave this field same as GOTRUE_SMTP_USER
GOTRUE_SMTP_ADMIN_EMAIL=user1@example.com

# This is the email account that is the admin account
# which has the highest privilege level, typically use to
# manage other users, such as user creation, deletion, password change, etc
GOTRUE_ADMIN_EMAIL=admin@example.com
GOTRUE_ADMIN_PASSWORD=password

# This is the address of the authentication server
# which is the same as the public IP/hostname of your host server
# when an email confirmation link is click, this is the host that user's devices
# will try to connect to
API_EXTERNAL_URL=http://your-host

# File Storage
# This affects where the files will be uploaded.
# By default, Minio will be deployed as file storage server which will use the host's disk storage.
# You can also AWS S3 by setting USE_MINIO as false and configure the AWS related fields.
USE_MINIO=true                    # determine if minio-server is used
# MINIO_URL=http://localhost:9000 # change this to use minio from a different host (e.g. maybe you self host Minio somewhere)
AWS_ACCESS_KEY_ID=minioadmin
AWS_SECRET_ACCESS_KEY=minioadmin
AWS_S3_BUCKET=appflowy
AWS_REGION=us-east-1              # This option only applicable for AWS S3
```

For authentication details, refer to the [Authentication](./AUTHENTICATION.md) documentation. You will need to update the
redirect URI to match your host server's public IP or hostname, such as `http://<your-host-server-public-ip-or-hostname>/callback`.
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
- The following command will build and start the AppFlowy-Cloud.

```bash
docker compose up -d
```
- Please check that all the services are running
```bash
docker ps -a
```

### 4. Optional Services
There are optional services that are non essential in core functionalities of AppFlowy Cloud, there can be useful for administrative or debugging purpose.
The files containing these services are in `docker-compose-extra.yml`.
- `pgadmin` (Web UI configured easy view into deployed postgres database)
- `portainer`/`portainer_init` (Web UI for providing some monitoring and ease of container management)
- `tunnel` (cloud flare tunnel: provide secure way to connect appflowy to Cloudflare without a publicly routable IP address)
- `admin_frontend` (admin portal to manage accounts and adding authentication method, recommended to keep)
If you wish to deploy those, edit this file accordingly and do:
```
docker compose --file docker-compose-extra.yml up -d
```
You may ignore the orphan containers warning message from docker


> When using the `docker compose up -d` command without specifying a tag, Docker Compose will pull the `latest`
tag for the `appflowy_cloud` and `admin_frontend` images from Docker Hub by default. If you've set the `BACKEND_VERSION`
environment variable, it will pull the specified version instead. If `BACKEND_VERSION` is not set, Docker Compose
defaults to using the `latest` tag.

- Check that services are running correctly `docker ps -a`
- If you find a particular service not working properly, you can inspect the logs:
```bash
# Getting logs for a particular docker compose service
# You can obtain name by `docker ps -a`
docker logs <NAME>
# e.g. docker logs appflowy-cloud-admin_frontend-1
```

### 5. Reconfiguring and redeployment
- It is very common to reconfigure and restart. To do so, simply edit the `.env` and do `docker compose up -d` again

## Ports
- After Deployment, you should see that AppFlowy-Cloud is serving 2 ports
- `443` (https)
- `80`  (http)
- Your host server need to expose either of the port

## SSL Certificate
- To use your own SSL certications for https, replace `certificate.crt` and `private_key.key`
with your own in `nginx/ssl/` directory

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
