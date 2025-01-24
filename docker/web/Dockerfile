FROM node:20.12.0 AS builder

WORKDIR /app

ARG VERSION=main

RUN npm install -g pnpm@8.5.0
RUN git clone --depth 1 --branch ${VERSION} https://github.com/AppFlowy-IO/AppFlowy-Web.git .
RUN pnpm install
ENV AF_BASE_URL=AF_BASE_URL_PLACEHOLDER
ENV AF_GOTRUE_URL=AF_GOTRUE_URL_PLACEHOLDER
RUN pnpm run build

FROM nginx:alpine
COPY --from=builder /app/dist /usr/share/nginx/html/
COPY nginx.conf /etc/nginx/nginx.conf
COPY env.sh /docker-entrypoint.d/env.sh
RUN chmod +x /docker-entrypoint.d/env.sh
