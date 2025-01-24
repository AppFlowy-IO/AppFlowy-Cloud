#!bin/sh
STATIC_JS_FILE_PATH="/usr/share/nginx/html/static/js"
if [ -z "${AF_BASE_URL}" ]; then
  echo "Error: AF_BASE_URL is not set."
  exit 1
fi
if [ -z "${AF_GOTRUE_URL}" ]; then
  echo "Error: AF_BASE_URL is not set."
  exit 1
fi
find ${STATIC_JS_FILE_PATH} -type f -exec sed -i "s|AF_BASE_URL_PLACEHOLDER|$AF_BASE_UR|g" {} +
find ${STATIC_JS_FILE_PATH} -type f -exec sed -i "s|AF_GOTRUE_URL_PLACEHOLDER|$AF_GOTRUE_URL|g" {} +
