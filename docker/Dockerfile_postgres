FROM postgres:latest

# Install dependencies required for pgjwt
RUN apt-get update && \
    apt-get install -y build-essential postgresql-server-dev-all git

# Clone and build pgjwt
RUN rm -rf pgjwt && \
    git clone https://github.com/michelp/pgjwt.git && \
    cd pgjwt && \
    make && \
    make install