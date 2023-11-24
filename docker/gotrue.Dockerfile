FROM golang
WORKDIR /go/src/supabase
RUN git clone https://github.com/supabase/gotrue.git
WORKDIR /go/src/supabase/gotrue
RUN git e67a10c1 && go install
CMD ["gotrue"]
