FROM golang as base
WORKDIR /go/src/supabase
RUN git clone https://github.com/supabase/gotrue.git --depth 1 --branch v2.117.0
WORKDIR /go/src/supabase/gotrue
COPY docker/gotrue.patch .
RUN git apply gotrue.patch
RUN CGO_ENABLED=0 go build -o /gotrue .

FROM scratch
COPY --from=base /usr/share/zoneinfo /usr/share/zoneinfo
COPY --from=base /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=base /etc/passwd /etc/passwd
COPY --from=base /etc/group /etc/group

COPY --from=base /gotrue .
COPY --from=base /go/src/supabase/gotrue/migrations ./migrations

CMD ["./gotrue"]
