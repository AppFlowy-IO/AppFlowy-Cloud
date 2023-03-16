-- Add migration script here
CREATE TABLE users
(
    uid         bigint PRIMARY KEY,
    username    TEXT        NOT NULL,
    password    TEXT        NOT NULL,
    email       TEXT        NOT NULL UNIQUE,
    create_time timestamptz NOT NULL
);