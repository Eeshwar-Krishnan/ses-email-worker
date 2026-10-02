export interface Env {
  /** R2 bucket that stores the raw inbound MIME messages. */
  EMAILS: R2Bucket;
  /** Queue the bridge Lambda publishes metadata to. */
  EMAIL_QUEUE: Queue<EmailMetadata>;

  /** Shared HMAC secret, identical to the Lambda's `BRIDGE_HMAC_SECRET`. */
  BRIDGE_HMAC_SECRET: string;
  /** R2 S3 API token credentials, used only to presign PUT URLs. */
  R2_ACCESS_KEY_ID: string;
  R2_SECRET_ACCESS_KEY: string;

  R2_ACCOUNT_ID: string;
  R2_BUCKET: string;
  R2_KEY_PREFIX?: string;
  /** Maximum accepted clock skew for signed requests, in seconds. Defaults to 300. */
  ALLOWED_SKEW_SECONDS?: string;
  /** Presigned PUT URL lifetime, in seconds. Defaults to 3600. */
  PRESIGN_TTL_SECONDS?: string;
}

/** Metadata produced by the bridge Lambda for each inbound email. */
export interface EmailMetadata {
  /** SES message id, also the S3 object key. */
  messageId: string;
  /** R2 object key holding the raw MIME message. */
  r2Key: string;
  s3Bucket: string;
  s3Key: string;
  awsRegion: string;
  from: string | null;
  recipients: string[];
  to: string[];
  cc: string[];
  subject: string | null;
  date: string | null;
  headerMessageId: string | null;
  size: number;
  sha256: string;
  receivedAt: string;
}
