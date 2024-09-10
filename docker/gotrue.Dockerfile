FROM golang as base
WORKDIR /go/src/supabase
RUN git clone https://github.com/supabase/auth.git --depth 1 --branch v2.159.1
WORKDIR /go/src/supabase/auth
COPY docker/gotrue.patch .
RUN git apply gotrue.patch
RUN CGO_ENABLED=0 go build -o /auth .

FROM scratch
COPY --from=base /usr/share/zoneinfo /usr/share/zoneinfo
COPY --from=base /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=base /etc/passwd /etc/passwd
COPY --from=base /etc/group /etc/group

COPY --from=base /auth .
COPY --from=base /go/src/supabase/auth/migrations ./migrations

CMD ["./auth"]
