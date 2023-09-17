#!/bin/bash

# Generate key file
openssl genpkey -algorithm RSA \
    -pkeyopt rsa_keygen_bits:4096 \
    -pkeyopt rsa_keygen_pubexp:65537 | \
    # pkcs8
      # PKCS (Public-Key Cryptography Standards) is a series of standards developed by RSA
      # Laboratories that define formats for cryptographic objects, including private keys,
      # public keys, certificates, and messages.
    #-topk8
      # option specifies that the output should be in PKCS#8 format
    openssl pkcs8 -topk8 -nocrypt -outform pem -out appflowy.io.key

# Generate CSR file
#https://www.digicert.com/kb/csr-creation.htm
#https://www.digicert.com/kb/ssl-support/openssl-quick-reference-guide.htm
openssl req -subj "/C=US/ST=California/L=Sunnyvale/O=AppFlowy,Inc./CN=appflowy.io"\
    -new -days 3650 -key appflowy.io.key -out appflowy.io.csr

# Generate self-sign file
openssl x509 -req -days 365 -in appflowy.io.csr -signkey appflowy.io.key -out appflowy.io.crt

# Verify certificate signing request
openssl req -text -noout -verify -in appflowy.io.csr

# verify certificate
openssl x509 -text -noout -in appflowy.io.crt
