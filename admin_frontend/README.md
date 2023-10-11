# Admin Frontend

- Start the whole stack: `docker compose up -d`
- Go to [web server](localhost)
- Quick rebuild only frontend after editing `docker compose up -d --no-deps --build admin_frontend`
- You might need to add `--force-recreate` for non build changes to take effect
