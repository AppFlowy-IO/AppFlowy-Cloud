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
- Modify the values in `.env` according to your needs. Minimally, you will have to update $FQDN
  to match your host server's domain, unless you are deploying on localhost.

If you would like to use one of the identity providers to log in, refer to the [Authentication](./AUTHENTICATION.md) documentation.

If you would like to use magic link to log in, you will need to set up env variables related to SMTP.

If neither of the above are configured, then the only way to sign in is via the admin portal (the home page), using the admin email
and password. After logging in as an admin, you can add users and set their passwords. The new user will be able to login to the admin
portal using this credential.

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
We include all services in the file `docker-compose.yml`. It is easier to start all services and remove orphan containers warning message.

- `pgadmin` (Web UI to visualize the provided postgres database)
- `tunnel` (Cloudflare tunnel to provide a secure way to connect AppFlowy to Cloudflare without a publicly routable IP
  address)

```
docker compose --file docker-compose-extras.yml up -d
```


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

### 6. Upgrading the services

- To upgrade to the latest version, use `docker compose pull` and `git pull` to fetch the latest changes for
  images, docker compose files, and the configuration files.
- Then, run `docker compose up -d` to start the services.
- Alternatively, you can use a specific image tag instead of `latest`, and checkout the corresponding tag for
  the repository.
- If you found that `nginx.conf` or `deploy.env` has been updated, please update your env and nginx configuration based on the change.
- Sometimes there might be additional steps required for upgrade. Refer to the [upgrade notes](https://appflowy.com/docs/self-hosters-upgrade-notes)
  for more information.

### 7. AppFlowy Web
- AppFlowy Web is provided as part of the docker compose setup, and should work out of the box. It is accessible via `/app` or `/`.
- For existing self hosters who upgraded their setup to include AppFlowy Web in the docker compose, `/` might redirect them to `/web`
  instead of `/app`. This is because the home page `/` used to be occupied by the admin console, and redirects to `/web` by default.
  The browser cache might need to be cleared to see the new behavior. Alternatively, just access the AppFlowy Web directly via `/app`.

- In order for login flow to succeed, we need to make sure that the necessary headers for redirect url can be passed
  to AppFlowy Cloud. If you are using only the Nginx service running within the official docker compose setup, then
  this is already taken care of and no further steps are required. Otherwise, if you have an external Nginx in front of
  the service, then make sure that you have the following:
  ```
  proxy_pass_request_headers on;
  underscores_in_headers on;
  ```
- If AppFlowy Web is served on a separate domain, you will need to modify the nginx conf to prevent CORS issues.
  By default, we allow requests from `localhost:3000`, using the configuration below:
  ```
  map $http_origin $cors_origin {
    # AppFlowy Web origin
    "~^http://localhost:3000$" $http_origin;
    default "null";
  }
  ```
  Replace `http://localhost:3000` with your AppFlowy Web origin.
- If you wish to build you own AppFlowy Web docker image, then run the following commands from the root directory of this repository:
  ```
  docker build --build-arg VERSION=v<insert version here> docker/web -f docker/web/Dockerfile -t appflowy-web
  ```
  The available versions can be found on [AppFlowy Web repository](https://github.com/AppFlowy-IO/AppFlowy-Web).


## Ports

- After Deployment, you should see that AppFlowy-Cloud is serving 2 ports
- `443` (https)
- `80`  (http)
- Your host server need to expose either of these ports.

## SSL Certificate

- To use your own SSL certificates for https, replace `certificate.crt` and `private_key.key`
  with your own in `nginx/ssl/` directory. Please note that the certificates in the repository are
  for demonstration purpose only and will need to be replaced by a certificate that is trusted by your devices.
  For example, you can use [Let's Encrypt](https://letsencrypt.org/), or CloudFlare Origin CA, if the AppFlowy
  Cloud endpoint is placed behind a cloudflare proxy.

## Usage of AppFlowy Application with AppFlowy Cloud

- [AppFlowy with AppFlowyCloud](https://docs.appflowy.io/docs/guides/appflowy/self-hosting-appflowy)

## FAQ

### How do I use a different `postgres`?
  The default url is using the postgres in docker compose, in service `appflowy_cloud` and `gotrue` respectively.
  However it is possible to use an external postgres, as long as it is accessible by the services. pgvector extension must also be installed.

- You need to change the following settings:
```
POSTGRES_HOST=postgres
POSTGRES_USER=postgres
POSTGRES_PASSWORD=password
POSTGRES_PORT=5432
POSTGRES_DB=postgres
```

### How do I disable signups?

If your deployed AppFlowy-Cloud is publicly available and you do not want any other users to access it, you can disable sign up
by setting the `GOTRUE_DISABLE_SIGNUP` environment variable to `true`.

### What port should I use for SMTP?

The default configuration assumes that TLS is used for SMTP, typically on port 465. If you are using STARTTLS, such as when
using port 587, please change `APPFLOWY_MAILER_SMTP_TLS_KIND` to `opportunistic`.

### What functionality will I lose if the SMTP server is not set up?

Sign in via magic link will not be possible. Inviting users to workspace and accepting invitation will have to be
performed via the admin portal as opposed to links provided in emails.

### I already have an Nginx server running on my host server. How do I configure it to work with AppFlowy-Cloud?
- First, remove the `nginx` service from the `docker-compose.yml` file.
- Update the docker compose file such that the ports for `appflowy_cloud`, `gotrue`, and `admin_frontend` are mapped to the different ports on the host server. If possible, use firewall to make sure that these ports are not accessible
  from the internet.
- An example site configuration for Nginx has been provided in `external_proxy_config/nginx/appflowy.site.conf`. You can use this as a starting point for your own configuration, and include this in nginx.conf of your external Nginx server.

### I am using Nginx Proxy Manager (or similar UI based proxy manager), and I am not able to replicate the configuration in external_proxy_config/nginx/appflowy.site.conf easily.
- Another alternative is to keep the `nginx` service in the `docker-compose.yml` file, then pass all the requests from the proxy manager to the `nginx` service. You have to turn on `Websockets Support`, or any equivalent options that adds a connection upgrade header.

### AppFlowy Web keeps redirecting to the desktop application after login.
- Refer to the AppFlowy Web section in the deployment steps. Make sure that the necessary headers are present.
