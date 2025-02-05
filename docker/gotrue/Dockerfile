FROM golang as base
WORKDIR /go/src/supabase
RUN git clone https://github.com/supabase/auth.git --depth 1 --branch v2.159.1
WORKDIR /go/src/supabase/auth
COPY patch/mfa_enabled.patch .
RUN git apply mfa_enabled.patch
RUN CGO_ENABLED=0 go build -o /auth .
RUN rm /go/src/supabase/auth/migrations/20240612123726_enable_rls_update_grants.up.sql

FROM alpine:3.20
RUN adduser -D -u 1000 supabase

RUN apk add --no-cache ca-certificates curl
USER supabase

COPY --from=base /auth .
COPY --from=base /go/src/supabase/auth/migrations ./migrations

COPY start.sh .
CMD ["./start.sh"]
