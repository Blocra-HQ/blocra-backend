#!/bin/bash

# Production Setup Helper Script
# This script helps you configure production environment variables

echo "🚀 BlocRA Production Setup Helper"
echo "=================================="
echo ""

# Generate JWT Secret
echo "📝 Generating JWT Secret..."
JWT_SECRET=$(openssl rand -base64 32)
echo "✅ JWT Secret generated: $JWT_SECRET"
echo ""

# Get backend URL
echo "🔗 Enter your Render backend URL (e.g., https://your-app.onrender.com):"
read BACKEND_URL

if [ -z "$BACKEND_URL" ]; then
    echo "❌ Backend URL is required!"
    exit 1
fi

echo ""
echo "📋 RENDER ENVIRONMENT VARIABLES"
echo "================================"
echo "Copy these to your Render service → Environment tab:"
echo ""
echo "DATABASE_URL=sqlite:/tmp/blocra.db"
echo "JWT_SECRET=$JWT_SECRET"
echo "RUST_LOG=info"
echo "PORT=5000"
echo "CORS_ORIGIN=https://blocrahq.vercel.app"
echo ""

echo "📋 VERCEL ENVIRONMENT VARIABLES"
echo "================================"
echo "Copy these to your Vercel project → Settings → Environment Variables:"
echo ""
echo "VITE_BACKEND_URL=$BACKEND_URL"
echo "VITE_STARKNET_RPC_URL=https://rpc.starknet.lava.build"
echo ""

echo "✅ Setup complete!"
echo ""
echo "Next steps:"
echo "1. Add the Render environment variables in Render dashboard"
echo "2. Redeploy your Render service"
echo "3. Add the Vercel environment variables in Vercel dashboard"
echo "4. Redeploy your Vercel project"
echo "5. Test your production app!"
echo ""
echo "📖 See PRODUCTION_DEPLOYMENT.md for detailed instructions"
