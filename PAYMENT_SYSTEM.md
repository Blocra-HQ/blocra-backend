# Payment System Documentation

## Overview
BlocRA implements a flexible payment system for EDA (Exploratory Data Analysis) report generation with two pricing models:

1. **Pay-per-use**: $3 per report (one-time payment)
2. **Monthly subscription**: $50/month for 100 reports

## Database Schema

### Tables Created
- `subscription_plans` - Available pricing plans
- `user_subscriptions` - Active user subscriptions
- `payment_transactions` - Payment history
- `report_usage` - Report generation tracking

## API Endpoints

### Public Endpoints

#### GET `/api/payments/plans`
Get all available subscription plans.

**Response:**
```json
{
  "success": true,
  "plans": [
    {
      "id": 1,
      "name": "Pay Per Report",
      "price_cents": 300,
      "report_limit": 1,
      "duration_days": 0,
      "description": "One-time payment for a single EDA report"
    },
    {
      "id": 2,
      "name": "Monthly Pro",
      "price_cents": 5000,
      "report_limit": 100,
      "duration_days": 30,
      "description": "Monthly subscription with 100 EDA reports"
    }
  ]
}
```

### Protected Endpoints (Require Authentication)

#### GET `/api/payments/status`
Get user's current subscription status.

**Headers:**
```
Authorization: Bearer <jwt_token>
```

**Response:**
```json
{
  "has_active_subscription": true,
  "subscription": {
    "id": 1,
    "user_id": 123,
    "plan_id": 2,
    "status": "active",
    "reports_used": 15,
    "reports_limit": 100,
    "start_date": "2024-01-01T00:00:00Z",
    "end_date": "2024-01-31T23:59:59Z"
  },
  "reports_remaining": 85,
  "can_generate_report": true
}
```

#### POST `/api/payments/one-time`
Purchase a single report ($3).

**Headers:**
```
Authorization: Bearer <jwt_token>
Content-Type: application/json
```

**Request Body:**
```json
{
  "plan_id": 1,
  "payment_method_id": "pm_card_visa"
}
```

**Response:**
```json
{
  "success": true,
  "transaction_id": 456,
  "client_secret": "pi_sim_abc123",
  "message": "Payment successful. You can now generate 1 report."
}
```

#### POST `/api/payments/subscribe`
Subscribe to monthly plan ($50/month for 100 reports).

**Headers:**
```
Authorization: Bearer <jwt_token>
Content-Type: application/json
```

**Request Body:**
```json
{
  "plan_id": 2,
  "payment_method_id": "pm_card_visa"
}
```

**Response:**
```json
{
  "success": true,
  "transaction_id": 789,
  "client_secret": "sub_sim_xyz789",
  "message": "Subscription activated! You have 100 reports available."
}
```

#### POST `/api/payments/cancel`
Cancel active subscription.

**Headers:**
```
Authorization: Bearer <jwt_token>
Content-Type: application/json
```

**Request Body:**
```json
{
  "subscription_id": 1
}
```

**Response:**
```json
{
  "success": true,
  "message": "Subscription cancelled successfully"
}
```

#### POST `/api/payments/check-usage`
Check if user can generate a report and track usage.

**Headers:**
```
Authorization: Bearer <jwt_token>
Content-Type: application/json
```

**Request Body:**
```json
{
  "contract_address": "0x1234567890abcdef"
}
```

**Response (Subscription):**
```json
{
  "success": true,
  "can_generate": true,
  "payment_type": "subscription",
  "reports_remaining": 84,
  "message": "Report generation authorized. 84 reports remaining."
}
```

**Response (One-time):**
```json
{
  "success": true,
  "can_generate": true,
  "payment_type": "one_time",
  "message": "Report generation authorized (one-time payment)."
}
```

**Error Response:**
```json
{
  "error": "No valid payment or subscription found. Please purchase a report or subscribe."
}
```

#### GET `/api/payments/history`
Get user's payment transaction history.

**Headers:**
```
Authorization: Bearer <jwt_token>
```

**Response:**
```json
{
  "success": true,
  "transactions": [
    {
      "id": 1,
      "user_id": 123,
      "amount_cents": 5000,
      "currency": "USD",
      "payment_type": "subscription",
      "status": "completed",
      "created_at": "2024-01-01T00:00:00Z"
    }
  ]
}
```

#### GET `/api/payments/reports`
Get user's report generation history.

**Headers:**
```
Authorization: Bearer <jwt_token>
```

**Response:**
```json
{
  "success": true,
  "reports": [
    {
      "id": 1,
      "user_id": 123,
      "contract_address": "0x1234567890abcdef",
      "report_type": "eda",
      "payment_type": "subscription",
      "created_at": "2024-01-15T10:30:00Z"
    }
  ]
}
```

## Integration Flow

### For Frontend Integration

1. **Display Pricing Plans**
   ```typescript
   const plans = await fetch('/api/payments/plans').then(r => r.json());
   ```

2. **Check User Status**
   ```typescript
   const status = await fetch('/api/payments/status', {
     headers: { 'Authorization': `Bearer ${token}` }
   }).then(r => r.json());
   ```

3. **Before Generating Report**
   ```typescript
   try {
     const check = await fetch('/api/payments/check-usage', {
       method: 'POST',
       headers: {
         'Authorization': `Bearer ${token}`,
         'Content-Type': 'application/json'
       },
       body: JSON.stringify({ contract_address: '0x...' })
     }).then(r => r.json());
     
     if (check.can_generate) {
       // Proceed with report generation
       generateReport();
     }
   } catch (error) {
     // Show payment modal
     showPaymentModal();
   }
   ```

4. **Purchase Single Report**
   ```typescript
   const payment = await fetch('/api/payments/one-time', {
     method: 'POST',
     headers: {
       'Authorization': `Bearer ${token}`,
       'Content-Type': 'application/json'
     },
     body: JSON.stringify({
       plan_id: 1,
       payment_method_id: stripePaymentMethodId
     })
   }).then(r => r.json());
   ```

5. **Subscribe to Monthly Plan**
   ```typescript
   const subscription = await fetch('/api/payments/subscribe', {
     method: 'POST',
     headers: {
       'Authorization': `Bearer ${token}`,
       'Content-Type': 'application/json'
     },
     body: JSON.stringify({
       plan_id: 2,
       payment_method_id: stripePaymentMethodId
     })
   }).then(r => r.json());
   ```

## Stripe Integration (Production)

### Setup Required

1. **Install Stripe SDK**
   ```bash
   cargo add stripe-rust
   ```

2. **Environment Variables**
   ```env
   STRIPE_SECRET_KEY=sk_live_...
   STRIPE_PUBLISHABLE_KEY=pk_live_...
   STRIPE_WEBHOOK_SECRET=whsec_...
   ```

3. **Update Payment Handler**
   Replace simulation code with actual Stripe API calls:
   ```rust
   use stripe::{Client, CreatePaymentIntent, Currency};
   
   let client = Client::new(env::var("STRIPE_SECRET_KEY")?);
   let payment_intent = CreatePaymentIntent::new(
       plan.price_cents,
       Currency::USD
   );
   ```

4. **Webhook Handler**
   Create endpoint to handle Stripe webhooks for:
   - Payment success/failure
   - Subscription renewal
   - Subscription cancellation

## Security Considerations

1. **Authentication Required**: All payment endpoints (except `/plans`) require valid JWT
2. **User Verification**: Subscriptions are tied to user_id from JWT claims
3. **Transaction Tracking**: All payments logged with timestamps
4. **Usage Limits**: Enforced at database level with counters
5. **Idempotency**: Prevent duplicate charges for same report

## Testing

### Test One-Time Payment
```bash
curl -X POST http://localhost:5000/api/payments/one-time \
  -H "Authorization: Bearer YOUR_JWT_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"plan_id": 1, "payment_method_id": "pm_test"}'
```

### Test Subscription
```bash
curl -X POST http://localhost:5000/api/payments/subscribe \
  -H "Authorization: Bearer YOUR_JWT_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"plan_id": 2, "payment_method_id": "pm_test"}'
```

### Test Report Generation Check
```bash
curl -X POST http://localhost:5000/api/payments/check-usage \
  -H "Authorization: Bearer YOUR_JWT_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"contract_address": "0x1234567890abcdef"}'
```

## Migration

Run the migration to create payment tables:
```bash
cd backend-rust
sqlx migrate run
```

## Future Enhancements

1. **Stripe Integration**: Replace simulation with real Stripe API
2. **Webhook Handling**: Auto-renewal and payment failure notifications
3. **Proration**: Handle mid-cycle subscription changes
4. **Discounts**: Coupon code support
5. **Team Plans**: Multi-user subscriptions
6. **Usage Analytics**: Detailed reporting dashboard
7. **Email Notifications**: Payment receipts and renewal reminders
8. **Refund System**: Handle refund requests
9. **Invoice Generation**: PDF invoices for payments
10. **Multiple Payment Methods**: Support for crypto payments
