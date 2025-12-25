#!/bin/bash

# Simple API test script for Rust backend

BASE_URL="http://localhost:5000"

echo "🧪 Testing BlocRA Rust Backend API"
echo "=================================="
echo ""

# Test 1: Health Check
echo "1️⃣  Testing health endpoint..."
response=$(curl -s -w "\n%{http_code}" "$BASE_URL/health")
http_code=$(echo "$response" | tail -n1)
body=$(echo "$response" | head -n-1)

if [ "$http_code" = "200" ]; then
    echo "✅ Health check passed"
    echo "   Response: $body"
else
    echo "❌ Health check failed (HTTP $http_code)"
    exit 1
fi
echo ""

# Test 2: Root endpoint
echo "2️⃣  Testing root endpoint..."
response=$(curl -s -w "\n%{http_code}" "$BASE_URL/api/")
http_code=$(echo "$response" | tail -n1)
body=$(echo "$response" | head -n-1)

if [ "$http_code" = "200" ]; then
    echo "✅ Root endpoint passed"
    echo "   Response: $body"
else
    echo "❌ Root endpoint failed (HTTP $http_code)"
fi
echo ""

# Test 3: List bounties (should work even without auth)
echo "3️⃣  Testing bounties list..."
response=$(curl -s -w "\n%{http_code}" "$BASE_URL/api/bounties")
http_code=$(echo "$response" | tail -n1)
body=$(echo "$response" | head -n-1)

if [ "$http_code" = "200" ]; then
    echo "✅ Bounties list passed"
    echo "   Response: $body"
else
    echo "⚠️  Bounties list returned HTTP $http_code (may need auth)"
fi
echo ""

# Test 4: Auth endpoint (should return 401 without token)
echo "4️⃣  Testing auth endpoint..."
response=$(curl -s -w "\n%{http_code}" "$BASE_URL/api/auth/me")
http_code=$(echo "$response" | tail -n1)

if [ "$http_code" = "401" ]; then
    echo "✅ Auth protection working (401 as expected)"
else
    echo "⚠️  Auth returned HTTP $http_code"
fi
echo ""

echo "=================================="
echo "✅ Basic API tests complete!"
echo ""
echo "Next steps:"
echo "  - Test with real authentication"
echo "  - Create a bounty"
echo "  - Test all CRUD operations"
echo ""
echo "For detailed testing, use:"
echo "  curl -X POST $BASE_URL/api/auth/wallet \\"
echo "    -H 'Content-Type: application/json' \\"
echo "    -d '{\"wallet_address\":\"0x123...\"}'"
