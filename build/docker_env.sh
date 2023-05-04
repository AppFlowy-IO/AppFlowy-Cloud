#!/bin/bash
export BACKEND_VERSION="v0.0.1"

export POSTGRES_USER=postgres
export POSTGRES_PASSWORD=password
export POSTGRES_PORT=5432
export POSTGRES_HOST=appflowy_postgres
export POSTGRES_DB=appflowy_pg

export DATABASE_URL=postgres://${POSTGRES_USER}:${POSTGRES_PASSWORD}@${POSTGRES_HOST}:${POSTGRES_PORT}/${POSTGRES_DB}