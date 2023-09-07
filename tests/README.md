# Cloud Test

## Prerequisites
- docker-compose
- SMTP server
- enviromental variables
```bash
# put in .env in the same directory
GOTRUE_JWT_SECRET=some_secret_key # self defined
GOTRUE_SMTP_HOST=smtp.gmail.com   # using gmail smtp as example, change if needed
GOTRUE_SMTP_PORT=465
GOTRUE_SMTP_USER=email_sender@some_company.com
GOTRUE_SMTP_PASS=email_sender_password
GOTRUE_SMTP_ADMIN_EMAIL=comp_admin@@some_company.com
# GOTRUE_MAILER_AUTOCONFIRM false

GOTRUE_REGISTERED_EMAIL=your_email@some_company.com
GOTRUE_REGISTERED_PASSWORD=your_password
```

## Steps

### 1. Start the docker-compose
- `docker-compose up -d`

### 2. Create registered user

#### Manual
- send link to your email
```bash
curl localhost:9998/signup \
  --data-raw '{"email":"'"$GOTRUE_REGISTERED_EMAIL"'","password":"'"$GOTRUE_REGISTERED_PASSWORD"'"}' \
  --header 'Content-Type: application/json'
```
- click on the link

#### Auto
- skips the clicking on email
```bash
export GOTRUE_MAILER_AUTOCONFIRM=true
docker-compose up -d
source .env
curl localhost:9998/signup \
  --data-raw '{"email":"'"$GOTRUE_REGISTERED_EMAIL"'","password":"'"$GOTRUE_REGISTERED_PASSWORD"'"}' \
  --header 'Content-Type: application/json'

export GOTRUE_MAILER_AUTOCONFIRM=false
docker-compose up -d
```

### 3. Run test
- `cargo test`
