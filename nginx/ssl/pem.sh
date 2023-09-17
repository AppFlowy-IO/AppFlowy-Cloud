#!/bin/bash
openssl req -x509 -newkey rsa:4096 \
 -keyout key.pem -out cert.pem \
 -days 365 -nodes -subj "/C=US/ST=California/L=Sunnyvale/O=AppFlowy,Inc./CN=appflowy.io" \

openssl x509 -in cert.pem -text -noout