Build docker image for appflowy worker manually

```shell
docker buildx build -f ./services/appflowy-worker/Dockerfile  --platform linux/amd64 -t appflowyinc/appflowy_worker --push .
```