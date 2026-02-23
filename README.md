# GitHub Collaborator Audit & Removal Dashboard (Rust)

Production-oriented Rust web app for auditing and removing collaborators from repositories owned by the authenticated GitHub user.

## Required Environment Variables

- `GITHUB_CLIENT_ID`
- `GITHUB_CLIENT_SECRET`
- `SESSION_SECRET`
- `BASE_URL`

The app fails fast on startup when any required variable is missing.

## Local Run

```bash
cargo run
```

Server listens on `0.0.0.0:3000`.

## Docker

```bash
docker build -t collab-dashboard .
docker run --rm -p 3000:3000 \
  -e GITHUB_CLIENT_ID=... \
  -e GITHUB_CLIENT_SECRET=... \
  -e SESSION_SECRET=... \
  -e BASE_URL=https://your-app.example.com \
  collab-dashboard
```

## OAuth Callback

Set your OAuth app callback URL to:

`$BASE_URL/auth/callback`

## Routes

- `GET /` landing page
- `GET /auth/login` start GitHub OAuth
- `GET /auth/callback` OAuth callback
- `GET /dashboard` repository/collaborator dashboard
- `POST /remove` bulk collaborator removal JSON API
- `POST /logout` session termination
