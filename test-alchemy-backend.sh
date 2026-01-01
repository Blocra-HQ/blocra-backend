#!/bin/bash

echo "🧪 Testing Alchemy Backend Integration"
echo ""

# Check if backend is running
if ! curl -s http://localhost:5000/api/health > /dev/null; then
    echo "❌ Backend is not running!"
    echo "   Start it with: cd backend-rust && cargo run"
    exit 1
fi

echo "✅ Backend is running"
echo ""

# Test contract transactions endpoint
echo "Test: Fetch transactions for Base contract"
echo "Contract: 0xCd45aC05fe7C014D6B2F62b3446E2A91D661a236"
echo "Chain: base"
echo "Block range: 39,850,000 to 39,950,000"
echo ""

response=$(curl -s -X POST http://localhost:5000/api/contract/transactions \
  -H "Content-Type: application/json" \
  -d '{
    "contract_address": "0xCd45aC05fe7C014D6B2F62b3446E2A91D661a236",
    "chain": "base",
    "from_block": "0x25FE710",
    "to_block": "0x2616E97"
  }')

# Check if response is valid JSON and contains transactions
if echo "$response" | jq -e '.transactions' > /dev/null 2>&1; then
    count=$(echo "$response" | jq -r '.count')
    echo "✅ API returned response"
    echo "   Transactions found: $count"
    echo ""
    
    # Pretty print first transaction
    echo "Sample transaction:"
    echo "$response" | jq '.transactions[0]' 2>/dev/null
    echo ""
    
    echo "✅ All tests passed!"
    echo ""
    echo "🚀 Alchemy backend integration is working!"
else
    echo "❌ API returned error or invalid response:"
    echo "$response"
    exit 1
fi
