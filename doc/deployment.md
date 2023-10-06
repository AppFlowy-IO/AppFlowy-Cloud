# Deployment
- AppFlowy-Cloud is designed to be easily self deployed for self managed cloud storage
- The following document will walk you through the steps to deploy your own AppFlowy-Cloud

## Hardware Requirements
- Because AppFlowy-Cloud will have to be running persistently (or at least when one of the user is using),
we recommend using cloud compute services (as your host server) such as
 - [Amazon EC2](https://aws.amazon.com/ec2/) or
 - [Azure Virtual Machines](https://azure.microsoft.com/en-gb/products/virtual-machines/)
- Minimum 2GB Ram (4GB Recommended)
- Ports 80/443 available

## Software Requirements
- [docker compose](https://docs.docker.com/compose)
This is needed be installed in your host server

## Steps

### 1. Getting source files
- Clone this repository into your host server and `cd` into it
```bash
git clone https://github.com/AppFlowy-IO/AppFlowy-Cloud`
cd AppFlowy-Cloud`
```

### 2. Preparing the configuration
- This is perhaps the most important part of the deployment process, please read carefully.
- It is required that that is a `.env` file in the root directory of the repository.
- To get started, copy the template `dev.env` as `.env` using the following shell commands:
```bash
cp dev.env .env
```
- There will be values in the `.env` that needs to be change according to your needs
- Kindly read the following comments for each set of settings
```bash
# This is the secret key for authentication, please change this and keep the key safe
GOTRUE_JWT_SECRET=hello456

# This determine if the user will be user automatically be confirmed when they sign up
# If this is enabled, it requires a clicking a confirmation link in the email which user
# use for sign up.
# Pre-requisite if you enable: you need to have your SMTP Service set up,
# which you can then fill in the details below
GOTRUE_MAILER_AUTOCONFIRM=true

# if you enable mail confirmation, you need to set the SMTP configuration below
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
API_EXTERNAL_URL=http://localhost:9998

# 2 fields below are only relevant for development, can ignore
DATABASE_URL=postgres://postgres:password@localhost:5433/postgres
SQLX_OFFLINE=false

# Google OAuth2
# This enables login using user's google account
# To set up, you need to go the following sites:
# https://console.cloud.google.com/apis/credentials/consent
# https://console.cloud.google.com/apis/credentials -> create credentials -> create oauth client ID
# in the field `Authorised redirect URIs`, you should put `<your host server public ip/hostname>/callback`
GOTRUE_EXTERNAL_GOOGLE_ENABLED=false
GOTRUE_EXTERNAL_GOOGLE_CLIENT_ID=
GOTRUE_EXTERNAL_GOOGLE_SECRET=
GOTRUE_EXTERNAL_GOOGLE_REDIRECT_URI=http://localhost:9998/callback

# File Storage
# This affects where the files will be uploaded.
# By default, Minio will be deployed as file storage server # and it will use the host server's disk storage.
# You can also AWS S3 by setting USE_MINIO as false
USE_MINIO=true                    # determine if minio-server is used
# MINIO_URL=http://localhost:9000 # change this to use minio from a different host (e.g. maybe you self host Minio somewhere)
AWS_ACCESS_KEY_ID=minioadmin
AWS_SECRET_ACCESS_KEY=minioadmin
AWS_S3_BUCKET=appflowy
AWS_REGION=us-east-1              # This option only applicable for AWS S3
```

### 3. Running the services

### Start and run AppFlowy-Cloud
- The following command will build and start the AppFlowy-Cloud
```bash
docker compose up -d
```
- Please check that all the services are running
```bash
docker ps -a
```

### 4. Reconfiguring and redeployment
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
- [AppFlowy with AppFlowyCloud](./integration.md)
