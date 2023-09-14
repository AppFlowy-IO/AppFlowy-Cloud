FROM golang
WORKDIR /go/src/supabase
RUN git clone https://github.com/supabase/gotrue.git
RUN git checkout v2.95.2
WORKDIR /go/src/supabase/gotrue
RUN go install
CMD ["gotrue"]
