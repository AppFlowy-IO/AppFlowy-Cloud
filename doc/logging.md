# Logging
A simple guide to deploy the logging infrastructure for AppFlowy Cloud

## Components
- Listed in previous directory: `docker-compose-logging.yml`

## Quick Start
- This assumes that you have already deployed AppFlowy.
- In the previous directory, open a terminal and put the following command:
```
docker compose --file docker-compose-logging.yml up -d
```

## Credentials
- After deployment, when you go to localhost:5601, both username and password will be `admin`

## Web UI Setup
- In your browser, go to `localhost:5601`
- Login in with username and password
- Top Left -> Side Panel -> Dashboard Management -> Index Pattern -> Create
- Enter "logstash-logs-*" -> Add Timestamp as filter -> Create

## Example Usage
- Top Left -> Side Panel -> Discover

## Final Notes
This is a very simple guide to get started,
You might want to consider the following for security reasons:
- Changing default password
- Use Https protocol instead
- Clustering
For more information, go to Opensearch official website to find out more: [Opensearch](https://opensearch.org/docs/latest/security/configuration/security-admin/)
