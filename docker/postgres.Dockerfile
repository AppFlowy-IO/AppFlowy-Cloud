FROM postgres:latest

# Install dependencies required for pgjwt and locales
RUN apt-get update && \
    apt-get install --fix-missing -y \
        build-essential \
        postgresql-server-dev-all \
        git \
        locales && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

# Setup locale
RUN echo "en_US.UTF-8 UTF-8" > /etc/locale.gen && \
    locale-gen en_US.UTF-8 && \
    update-locale LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8

ENV LANG en_US.UTF-8
ENV LANGUAGE en_US:en
ENV LC_ALL en_US.UTF-8

# Clone and build pgjwt
RUN rm -rf pgjwt && \
    git clone https://github.com/michelp/pgjwt.git && \
    cd pgjwt && \
    make && \
    make install
