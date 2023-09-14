FROM golang
WORKDIR /go/src/supabase
RUN git clone https://github.com/supabase/gotrue.git
WORKDIR /go/src/supabase/gotrue
RUN git checkout v2.95.2 && go install
CMD ["gotrue"]
