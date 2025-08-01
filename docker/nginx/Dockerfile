FROM nginxinc/nginx-unprivileged:stable-alpine

COPY --chown=nginx nginx.conf /etc/nginx/conf.d/default.conf
COPY ssl/ /etc/nginx/ssl/
