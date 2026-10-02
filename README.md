# ses-email-worker

Route inbound email through AWS SES into the Cloudflare Workers ecosystem
(Email Workers, Queues, R2) using a small Rust Lambda as the bridge.

## Why

Cloudflare Email Routing's catch-all rule only works on apex domains. To route
mail for a subdomain you must list every literal address, so `*@tenant.example.com`
cannot be caught generically. This makes subdomain-per-tenant or wildcard
sub-addressing impossible to model with Email Routing alone.

AWS SES receipt rules do not have this limitation: a rule for `.example.com`
matches the domain *and all of its subdomains*, and rules can be written
without recipient conditions to match every verified identity. This project
puts SES in front for inbound mail and hands the message back to Cloudflare for
processing, so you keep writing your routing logic in a Worker.

## How it works

```
sender ──MX──▶ SES inbound ──receipt rule──▶ S3 object
                                                │
                                     S3 notification (ObjectCreated)
                                                ▼
                                    Rust Lambda (IAM: Get+Delete)
                                      │                     │
                        1. HMAC POST /presign               │
                                      ▼                     │
                        Cloudflare Worker ── presigned PUT  │
                                      │                     │
                        2. PUT raw message to R2 ◀──────────┘
                        3. Delete S3 object
                        4. HMAC POST /enqueue
                                      ▼
                        Cloudflare Queue ──▶ consumer Worker ▶ processEmail()
```

The Lambda never holds Cloudflare credentials. It authenticates to the Worker
with an HMAC-SHA256 shared secret; the Worker mints a short-lived presigned R2
URL and later accepts the queue message. Both directions are signed over
`method + path + timestamp + nonce + sha256(body)` and rejected outside a
configurable clock-skew window.

## Layout

```
crates/ses-bridge/   Rust Lambda: S3 -> R2 -> S3 delete -> queue
workers/bridge/      Cloudflare Worker: /presign, /enqueue, queue consumer
infra/               Example IAM policies, S3 notification and SES receipt rule
```

## Setup

### 1. Cloudflare

```bash
cd workers/bridge
npm install

npx wrangler r2 bucket create ses-inbound
npx wrangler queues create ses-inbound
```

Edit `wrangler.jsonc` (`R2_ACCOUNT_ID`, bucket/queue names, prefix), then create
an R2 API token with Object Read & Write for the bucket and set the secrets:

```bash
npx wrangler secret put BRIDGE_HMAC_SECRET        # openssl rand -hex 32
npx wrangler secret put R2_ACCESS_KEY_ID
npx wrangler secret put R2_SECRET_ACCESS_KEY
npx wrangler deploy
```

Note the deployed Worker URL, e.g. `https://ses-email-bridge.<subdomain>.workers.dev`.
Implement your email logic in `workers/bridge/src/email.ts` (`processEmail`); the
queue consumer already fetches the raw MIME message from R2 and calls it.

### 2. AWS

All SES receiving resources (rules, Lambda, KMS keys) must be in a
[region that supports SES email receiving](https://docs.aws.amazon.com/general/latest/gr/ses.html).
The S3 bucket may live elsewhere.

```bash
aws s3api create-bucket --bucket REPLACE_INBOUND_BUCKET --region REPLACE_REGION
```

Create the two IAM roles and attach the policies in `infra/iam/`:

- `lambda-role-trust.json` + `lambda-execution-policy.json` → the Lambda role.
- `ses-s3-role-trust.json` + `ses-s3-write-policy.json` → the SES write role.

Replace the `REPLACE_*` placeholders first.

### 3. Build and deploy the Lambda

```bash
cargo install cargo-lambda
(cd crates/ses-bridge && cargo lambda build --release --arm64 --output-format zip)

aws lambda create-function \
  --function-name ses-bridge \
  --runtime provided.al2023 \
  --architectures arm64 \
  --memory-size 128 \
  --timeout 30 \
  --handler bootstrap \
  --role arn:aws:iam::REPLACE_ACCOUNT_ID:role/ses-bridge-lambda \
  --zip-file fileb://target/lambda/ses-bridge/bootstrap.zip \
  --environment "Variables={BRIDGE_URL=https://...,BRIDGE_HMAC_SECRET=...}"
```

`BRIDGE_HMAC_SECRET` must match the Worker secret. `BRIDGE_URL` is the Worker
base URL. Optional: `R2_KEY_PREFIX` (default `inbound`), `PRESIGN_TTL_SECONDS`
(default `3600`).

### 4. Wire up the trigger and receipt rule

Allow S3 to invoke the function, attach the notification, and create the
receipt rule (a rule for `.example.com` catches every subdomain):

```bash
aws lambda add-permission --function-name ses-bridge \
  --statement-id s3-invoke --action lambda:InvokeFunction \
  --principal s3.amazonaws.com \
  --source-arn arn:aws:s3:::REPLACE_INBOUND_BUCKET

aws s3api put-bucket-notification-configuration \
  --bucket REPLACE_INBOUND_BUCKET \
  --notification-configuration file://infra/s3-notification.json

aws ses create-receipt-rule --cli-input-json file://infra/ses-receipt-rule.json
aws ses set-active-receipt-rule-set --rule-set-name default-rule-set
```

Finally point the domain's MX record (and each subdomain's MX) at the SES
inbound endpoints for your region, then send a test message.

## Operational notes

- The Lambda is idempotent on S3 deletes: a retried event whose object is gone
  is skipped. A duplicate `/enqueue` is possible on partial failure, so dedupe
  on `messageId` if your handler is not idempotent.
- Receipt rules match the SMTP envelope recipient (`RCPT TO`), which is why the
  Lambda reports `Delivered-To`/`X-Original-To` alongside `To`/`Cc`.
- `Subject` is forwarded as-is and may still be RFC 2047 encoded.

## License

MIT — see [LICENSE](LICENSE).
