# AppFlowy Self-Hosted — Coolify

[English](#english) | [Français](#français)

---

## English

### Overview
This configuration runs **AppFlowy Cloud** (self-hosted) using **Coolify** with:
- GoTrue for authentication (Supabase GoTrue)
- MinIO for S3-compatible storage
- Nginx as internal reverse proxy (critical for API routing)
- Redis for caching
- PostgreSQL (Supabase) for databases

### Prerequisites
- Coolify installed on your server
- A Supabase instance (or PostgreSQL 15+ with pgvector)
- A Redis instance
- An SMTP server for emails (optional but recommended)
- Two domain names pointing to your server:
  - `appflowy.yourdomain.com` — main app
  - `gotrue.yourdomain.com` — authentication

### File Structure
```text
/data/coolify/services/your-service/
├── docker-compose.yml # Main service definition
├── nginx-proxy.conf   # Nginx config (critical for /api/user/verify routing)
├── .env.example       # Environment variables template
└── README.md          # This file
```

### Installation

#### 1. Prepare the Supabase database
```sql
CREATE DATABASE appflowy_gotrue;
ALTER USER supabase_auth_admin WITH PASSWORD 'your_strong_password';
GRANT ALL PRIVILEGES ON DATABASE appflowy_gotrue TO supabase_auth_admin;
\c appflowy_gotrue
CREATE SCHEMA IF NOT EXISTS auth;
GRANT ALL ON SCHEMA auth TO supabase_auth_admin;
GRANT ALL ON SCHEMA public TO supabase_auth_admin;
```

#### 2. Create the AppFlowy database
```sql
CREATE DATABASE appflowy_cloud;
CREATE USER appflowy_user WITH PASSWORD 'your_password';
GRANT ALL PRIVILEGES ON DATABASE appflowy_cloud TO appflowy_user;
\c appflowy_cloud
CREATE EXTENSION IF NOT EXISTS vector;
```

#### 3. Configure environment variables
```bash
cp .env.example .env
nano .env
```

#### 4. Deploy with Coolify
1. Create a new **Docker Compose** service in Coolify
2. Copy `docker-compose.yml` and `nginx-proxy.conf` to the service directory
3. Set environment variables in Coolify UI
4. Deploy

#### 5. Verify
```bash
curl -si https://appflowy.yourdomain.com/api/server-info/auth-providers | head -4
# Expected: HTTP/2 200
```

### Architecture
```text
Browser
└── Traefik (Coolify, TLS)
    └── Nginx (appflowy_proxy:80)
        ├── /ws → appflowy_cloud:8000 (WebSocket)
        ├── /api/user/verify → appflowy_cloud:8000 (GET passthrough)
        ├── /api/ → appflowy_cloud:8000
        ├── /gotrue/ → gotrue:8081
        └── / → appflowy_web:80
```

### Key Fix — nginx-proxy.conf
The `GET /api/user/verify/{token}` endpoint must be routed **before** the generic
`/api/` block, as a simple passthrough — no method rewrite, no body transformation:
```nginx
location ~ ^/api/user/verify/(.+)$ {
    proxy_pass http://cloud/api/user/verify/$1;
    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
}
```

### Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `404` on login | Wrong nginx `verify` rule | Use GET passthrough (see Key Fix above) |
| `InvalidSignature` | Bad JWT secret format | Regenerate `AF_JWT_SECRET` in base64url |
| `Permission denied` on `auth` schema | Missing GRANT | Re-run step 1 SQL |
| GoTrue not responding | DNS or Traefik label | Check `gotrue.yourdomain.com` + Traefik labels |
| Blank page | Wrong `APPFLOWY_*_BASE_URL` | Match env vars to your actual domains |
| CORS errors | GoTrue URL mismatch | Check `APPFLOWY_GOTRUE_BASE_URL` |
| `appflowy_search` health error | Search service not deployed | Safe to ignore if not using AI search |

### Versions Tested
- AppFlowy Cloud: `appflowyinc/appflowy_cloud:latest`
- Supabase GoTrue: `v2.174.0`
- Coolify: `v4.0.0-beta.463`
- Nginx: `alpine`

---

## Without Coolify

If you are **not using Coolify**, you can still run this setup with plain `docker compose` and any reverse proxy (Nginx, Caddy, Traefik, etc.).

### Steps without Coolify:

1. Copy `.env.example` to `.env` and fill in your values
2. Run `docker compose up -d`
3. Configure your reverse proxy to route:
   - `https://appflowy.yourdomain.com` → `appflowy_proxy:80`
   - `https://gotrue.yourdomain.com` → `gotrue:8081`
4. Make sure to set proper TLS certificates for your domains

### Example Nginx configuration (standalone reverse proxy):
```nginx
# /etc/nginx/sites-available/appflowy
server {
    listen 443 ssl http2;
    server_name appflowy.yourdomain.com;
    
    ssl_certificate /etc/letsencrypt/live/appflowy.yourdomain.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/appflowy.yourdomain.com/privkey.pem;
    
    location / {
        proxy_pass http://localhost:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}

server {
    listen 443 ssl http2;
    server_name gotrue.yourdomain.com;
    
    ssl_certificate /etc/letsencrypt/live/gotrue.yourdomain.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/gotrue.yourdomain.com/privkey.pem;
    
    location / {
        proxy_pass http://localhost:8081;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

### Important Notes:
- The `nginx-proxy.conf` file is **still required** inside Docker even without Coolify
- It handles the internal routing between `appflowy_web`, `appflowy_cloud`, and `gotrue`
- You can remove the `traefik.enable=false` labels from the docker-compose.yml if not using Coolify
- Make sure all services are on the same Docker network

---

## Français

### Vue d'ensemble
Cette configuration exécute **AppFlowy Cloud** (auto-hébergé) avec **Coolify** :
- GoTrue pour l'authentification (Supabase GoTrue)
- MinIO pour le stockage compatible S3
- Nginx comme reverse proxy interne (critique pour le routage API)
- Redis pour le cache
- PostgreSQL (Supabase) pour les bases de données

### Prérequis
- Coolify installé sur votre serveur
- Une instance Supabase (ou PostgreSQL 15+ avec pgvector)
- Une instance Redis
- Un serveur SMTP (optionnel mais recommandé)
- Deux domaines pointant vers votre serveur :
  - `appflowy.votredomaine.com` — application principale
  - `gotrue.votredomaine.com` — authentification

### Structure des fichiers
```text
/data/coolify/services/votre-service/
├── docker-compose.yml # Définition des services
├── nginx-proxy.conf   # Config Nginx (critique pour /api/user/verify)
├── .env.example       # Modèle des variables d'environnement
└── README.md          # Ce fichier
```

### Installation

#### 1. Préparer la base Supabase
```sql
CREATE DATABASE appflowy_gotrue;
ALTER USER supabase_auth_admin WITH PASSWORD 'votre_mot_de_passe';
GRANT ALL PRIVILEGES ON DATABASE appflowy_gotrue TO supabase_auth_admin;
\c appflowy_gotrue
CREATE SCHEMA IF NOT EXISTS auth;
GRANT ALL ON SCHEMA auth TO supabase_auth_admin;
GRANT ALL ON SCHEMA public TO supabase_auth_admin;
```

#### 2. Créer la base AppFlowy
```sql
CREATE DATABASE appflowy_cloud;
CREATE USER appflowy_user WITH PASSWORD 'votre_mot_de_passe';
GRANT ALL PRIVILEGES ON DATABASE appflowy_cloud TO appflowy_user;
\c appflowy_cloud
CREATE EXTENSION IF NOT EXISTS vector;
```

#### 3. Variables d'environnement
```bash
cp .env.example .env
nano .env
```

#### 4. Déployer avec Coolify
1. Créer un nouveau service **Docker Compose** dans Coolify
2. Copier `docker-compose.yml` et `nginx-proxy.conf` dans le répertoire du service
3. Définir les variables dans l'interface Coolify
4. Déployer

#### 5. Vérifier
```bash
curl -si https://appflowy.votredomaine.com/api/server-info/auth-providers | head -4
# Attendu : HTTP/2 200
```

### Le fix clé — nginx-proxy.conf
L'endpoint `GET /api/user/verify/{token}` doit être routé **avant** le bloc
générique `/api/`, en simple passthrough — sans réécriture de méthode ni body :
```nginx
location ~ ^/api/user/verify/(.+)$ {
    proxy_pass http://cloud/api/user/verify/$1;
    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
}
```

### Dépannage

| Symptôme | Cause | Solution |
|----------|-------|----------|
| `404` à la connexion | Mauvaise règle nginx `verify` | Utiliser le GET passthrough (voir ci-dessus) |
| `InvalidSignature` | Mauvais format JWT | Régénérer `AF_JWT_SECRET` en base64url |
| `Permission denied` sur `auth` | GRANT manquant | Relancer le SQL de l'étape 1 |
| GoTrue ne répond pas | DNS ou label Traefik | Vérifier `gotrue.votredomaine.com` |
| Page blanche | `APPFLOWY_*_BASE_URL` incorrect | Faire correspondre les domaines réels |
| Erreurs CORS | URL GoTrue incorrecte | Vérifier `APPFLOWY_GOTRUE_BASE_URL` |
| Erreur `appflowy_search` health | Service search non déployé | Ignorable si search IA non utilisé |

### Versions testées
- AppFlowy Cloud : `appflowyinc/appflowy_cloud:latest`
- Supabase GoTrue : `v2.174.0`
- Coolify : `v4.0.0-beta.463`
- Nginx : `alpine`

---

## Without Coolify / Sans Coolify

### English
If you are **not using Coolify**, the setup still works with plain `docker compose` and any reverse proxy.

**Steps:**
1. Copy `.env.example` to `.env` and fill in your values
2. Run `docker compose up -d`
3. Configure your reverse proxy to route domains to the correct containers
4. Set up TLS certificates for your domains

**The `nginx-proxy.conf` file is still required** inside Docker — it handles internal routing between services.

### Français
Si vous **n'utilisez pas Coolify**, l'installation fonctionne aussi avec `docker compose` simple et un reverse proxy.

**Étapes :**
1. Copiez `.env.example` vers `.env` et renseignez vos valeurs
2. Exécutez `docker compose up -d`
3. Configurez votre reverse proxy pour router les domaines vers les bons conteneurs
4. Mettez en place les certificats TLS pour vos domaines

**Le fichier `nginx-proxy.conf` est toujours requis** à l'intérieur de Docker — il gère le routage interne entre les services.

---

## All-in-One Docker Compose (for testing only)

If you don't have an existing Supabase (PostgreSQL) or Redis instance, you can add them directly to the `docker-compose.yml`.  
**⚠️ Warning:** This will consume more resources (RAM, CPU, disk) and is **not recommended for production** unless you have no other option.

### Example addition to `docker-compose.yml`:

```yaml
services:
  # ... existing services (gotrue, minio, appflowy_cloud, appflowy_proxy, appflowy_web) ...

  postgres:
    image: postgres:15
    restart: unless-stopped
    environment:
      POSTGRES_DB: appflowy_cloud
      POSTGRES_USER: appflowy_user
      POSTGRES_PASSWORD: your_password
    volumes:
      - postgres_data:/var/lib/postgresql/data
    networks:
      - coolify

  redis:
    image: redis:alpine
    restart: unless-stopped
    command: redis-server --requirepass your_redis_password
    volumes:
      - redis_data:/data
    networks:
      - coolify

volumes:
  postgres_data:
  redis_data:
```

### Environment Updates
Update the environment variables in `.env` to point to these internal containers:
```env
AF_DB_URI=postgres://appflowy_user:your_password@postgres:5432/appflowy_cloud
AF_REDIS_URI=redis://:your_redis_password@redis:6379/4
SUPABASE_AUTH_PASSWORD=your_password  # (if you also use internal postgres for gotrue)
```

**Note:**  
- This duplicates services that could be shared across multiple applications.  
- Each additional container consumes memory (~200 MB for Postgres, ~50 MB for Redis).  
- You lose the benefits of a managed database (backups, high availability, etc.).  
- **For production, prefer external managed instances (Supabase, Neon, Aiven, etc.) or a shared self-hosted PostgreSQL/Redis.**

---

## License / Licence

MIT
