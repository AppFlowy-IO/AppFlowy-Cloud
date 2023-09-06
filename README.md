# AppFlowy Cloud
- Cloud Server for AppFlowy

## Deployment

### Environmental Variables before starting
- you can set it explicitly(below) or in a `.env` file (use `dev.env`) as template
```bash
# authentication key, change this and keep the key safe and secret
GOTRUE_JWT_SECRET=secret_auth_pass

# enabled by default, if you dont want need email confirmation, set to false
GOTRUE_MAILER_AUTOCONFIRM=true

# if you enable mail confirmation, you need to set the SMTP configuration below
GOTRUE_SMTP_HOST=smtp.gmail.com
GOTRUE_SMTP_PORT=465
GOTRUE_SMTP_USER=email_sender@some_company.com
GOTRUE_SMTP_PASS=email_sender_password
GOTRUE_SMTP_ADMIN_EMAIL=comp_admin@@some_company.com

# Change 'localhost' to the public host of machine that is running on.
# This is for email confirmation link
API_EXTERNAL_URL=http://localhost:9998
```
- additional settings can be modified in `docker-compose.yml`

### Start Cloud Server
```bash
docker-compose up -d
```

### Ports
Host Server is required to expose the following Ports:
- `8000`
- `9998`

## Local Development

### Pre-requisites

You'll need to install:

- [Rust](https://www.rust-lang.org/tools/install)
- [Docker](https://docs.docker.com/get-docker/)

### Configuration
- copy the configurations from `dev.env` to `.env`
- edit the `.env` as required (such as SMTP configurations)

### Run the dependency servers
```bash
docker-compose --file docker-compose-dev.yml up -d
```

### Install sqlx-cli
```bash
cargo install sqlx-cli
```

### Run sqlx migration
```bash
sqlx database create
sqlx migrate run
```

### Run the AppFlowy-Cloud server
```bash
cargo run
```

### Run the tests

#### Verified user
- Make sure you have registered a user and put into your `.env` file, else some test may fail
- You may register the email defined in `.env` by running the following command, you may then click on the link sent to that email to complete registration
```bash
source .env
curl localhost:9998/signup \
    --data-raw '{"email":"'"$GOTRUE_REGISTERED_EMAIL"'","password":"'"$GOTRUE_REGISTERED_PASSWORD"'"}' \
    --header 'Content-Type: application/json'
```
- Verify registration, you should get a token after running the command below:
```bash
source .env
curl localhost:9998/token?grant_type=password \
    --data-raw '{"email":"'"$GOTRUE_REGISTERED_EMAIL"'","password":"'"$GOTRUE_REGISTERED_PASSWORD"'"}' \
    --header 'Content-Type: application/json'
```

#### Test
```bash
cargo test
```
