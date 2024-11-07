FROM golang as base
WORKDIR /go/src/supabase
RUN git clone https://github.com/supabase/auth.git --depth 1 --branch v2.159.1
WORKDIR /go/src/supabase/auth
COPY patch/mfa_enabled.patch .
RUN git apply mfa_enabled.patch
RUN CGO_ENABLED=0 go build -o /auth .
RUN rm /go/src/supabase/auth/migrations/20240612123726_enable_rls_update_grants.up.sql

FROM scratch
COPY --from=base /usr/share/zoneinfo /usr/share/zoneinfo
COPY --from=base /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=base /etc/passwd /etc/passwd
COPY --from=base /etc/group /etc/group

COPY --from=base /auth .
COPY --from=base /go/src/supabase/auth/migrations ./migrations

CMD ["./auth"]
