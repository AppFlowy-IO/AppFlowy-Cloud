#!/usr/bin/env sh
curl -i -X POST \
	-H "Content-Type: application/json" \
	-d '{"Username": "admin", "Password": "'"$PORTAINER_PASSWORD"'"}' \
	http://portainer:9000/api/users/admin/init
