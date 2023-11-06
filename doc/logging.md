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
- Quick check: `docker ps -a`

## Common deployment problems
### Filebeat did not start successfully
```
$ docker logs appflowy-cloud-filebeat-1
Exiting: error loading config file: config file ("filebeat.yml") can only be writable by the owner but the permissions are "-rw-rw-r--" (to fix the permissions use: 'chmod go-w /usr/share/filebeat/filebeat.yml')
```
- Solution: remove write permission on the file: `chmod -w docker/filebeat/filebeat.yml`

### No Logs
- Observation: There are no logs in OpenSearch Dashboard
- Possibe Diagnostic: No read permission for `*.log` files in `/var/lib/docker/containers`

- One Time Solution: give read permission to docker logs
```
chmod -R a+r /var/lib/docker/containers
```
- Permanent Solution: give read permission to docker logs every time there's a modification
In the project root directory: `sudo ./docker/filebeat/grant_container_logs_permissions.sh`
  - Caveat: Only work on unix like operating system, requires `inotifywait`(`inotify-tools`) to be installed.
  MacOS alternative: `fswatch`

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
