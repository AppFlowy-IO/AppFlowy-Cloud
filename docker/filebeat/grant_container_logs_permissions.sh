#! /usr/bin/env bash

while true
do
	inotifywait /var/lib/docker/containers
	sleep 1
	sudo chmod -R a+r /var/lib/docker/containers
done
