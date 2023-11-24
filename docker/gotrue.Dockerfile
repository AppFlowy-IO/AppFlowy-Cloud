FROM golang as base
WORKDIR /go/src/supabase
RUN git clone https://github.com/supabase/gotrue.git
WORKDIR /go/src/supabase/gotrue
RUN git checkout e67a10c1 && \
	CGO_ENABLED=0 \
	GOOS=linux \
	GOARCH=amd64 \
	go build -o /gotrue .

FROM scratch
COPY --from=base /usr/share/zoneinfo /usr/share/zoneinfo
COPY --from=base /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=base /etc/passwd /etc/passwd
COPY --from=base /etc/group /etc/group

COPY --from=base /gotrue .

CMD ["./gotrue"]
