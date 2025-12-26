# Render Deployment Guide

## Quick Setup

### 1. Create Web Service on Render

1. Go to [Render Dashboard](https://dashboard.render.com/)
2. Click "New +" → "Web Service"
3. Connect your GitHub repository
4. Configure the service:

**Basic Settings:**
- **Name**: `blocra-backend` (or your preferred name)
- **Region**: Choose closest to your users
- **Branch**: `main`
- **Root Directory**: `backend-rust`
- **Runtime**: `Rust`
- **Build Command**: `cargo build --release`
- **Start Command**: `./render-start.sh` or `cargo run --release`

### 2. Environment Variables

Add these in Render Dashboard → Environment:

**Required:**
```bash
DATABASE_URL=sqlite:/tmp/blocra.db
JWT_SECRET=your_secure_random_string_here_min_32_chars
PORT=5000
```

**Optional:**
```bash
RUST_LOG=info
CORS_ORIGIN=https://yourdomain.com
RATE_LIMIT_MAX_REQUESTS=1000
RATE_LIMIT_WINDOW_SECS=900
```

### 3. Generate JWT Secret

Run this command locally and copy the output:
```bash
openssl rand -base64 32
```

Paste the result as your `JWT_SECRET` in Render.

## Important Notes

### Database Storage

⚠️ **CRITICAL: SQLite on Render uses `/tmp` which is EPHEMERAL!**

- Database is stored in `/tmp/blocra.db`
- **ALL USER DATA IS LOST on each deployment/restart**
- This is acceptable for:
  - Development/testing
  - Demo deployments
  - Stateless applications

**For Production (User Data Persistence):**

1. **Option 1: Render PostgreSQL (Recommended)**
   - Add PostgreSQL database in Render Dashboard
   - Update `DATABASE_URL` to PostgreSQL connection string
   - Requires code changes to support PostgreSQL

2. **Option 2: Render Persistent Disk (Paid Plans)**
   - Add persistent disk in Render Dashboard
   - Mount at `/data`
   - Set `DATABASE_URL=sqlite:/data/blocra.db`
   - Data persists across deployments

3. **Option 3: External Database**
   - Supabase (PostgreSQL)
   - PlanetScale (MySQL)
   - Turso (SQLite-compatible)
   - Set `DATABASE_URL` to external connection string

### Why /tmp?

Render's filesystem is read-only except for `/tmp`. The application automatically detects Render and uses `/tmp/blocra.db`.

## Troubleshooting

### "unable to open database file"

**Solution 1: Set DATABASE_URL explicitly**
```bash
DATABASE_URL=sqlite:/tmp/blocra.db
```

**Solution 2: Check logs**
```bash
# In Render dashboard, check logs for:
# - "Database URL: sqlite:/tmp/blocra.db"
# - "Running on Render platform"
```

**Solution 3: Use the start script**
Change Start Command to:
```bash
./render-start.sh
```

### Port Binding Issues

Render automatically sets the `PORT` environment variable. The app uses it by default.

If you see "No open ports detected":
- Ensure the app binds to `0.0.0.0:$PORT`
- Check that the server starts within 90 seconds

### Build Failures

If build times out:
- Render free tier has build time limits
- Consider upgrading to paid plan
- Or use pre-built Docker image

## Migration to PostgreSQL (Recommended for Production)

1. Add PostgreSQL database in Render
2. Update `Cargo.toml`:
   ```toml
   sqlx = { version = "0.8", features = ["runtime-tokio-rustls", "postgres"] }
   ```
3. Update connection code in `src/db.rs`
4. Set `DATABASE_URL` to PostgreSQL connection string

## Monitoring

- **Logs**: Render Dashboard → Logs tab
- **Metrics**: Render Dashboard → Metrics tab
- **Health Check**: Add `/health` endpoint monitoring

## Support

- [Render Documentation](https://render.com/docs)
- [Render Community](https://community.render.com/)
- [SQLx Documentation](https://docs.rs/sqlx/)
