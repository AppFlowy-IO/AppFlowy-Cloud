FROM dpage/pgadmin4

COPY ./docker/pgadmin/servers.json /pgadmin4/servers.json
COPY ./docker/pgadmin/custom_entrypoint.sh /custom_entrypoint.sh

USER root
RUN chmod +x /custom_entrypoint.sh

USER pgadmin

ENTRYPOINT ["/custom_entrypoint.sh"]
