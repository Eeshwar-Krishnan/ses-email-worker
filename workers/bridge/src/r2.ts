import { AwsClient } from "aws4fetch";
import type { Env } from "./types";

/**
 * Generates a presigned S3 `PutObject` URL for R2. The signature is computed
 * locally, so no R2 API call happens here.
 */
export async function presignPut(
  env: Env,
  key: string,
  contentType: string,
  expiresIn: number,
): Promise<string> {
  const client = new AwsClient({
    accessKeyId: env.R2_ACCESS_KEY_ID,
    secretAccessKey: env.R2_SECRET_ACCESS_KEY,
    service: "s3",
    region: "auto",
  });

  const url = new URL(
    `https://${env.R2_ACCOUNT_ID}.r2.cloudflarestorage.com/${env.R2_BUCKET}/${key}`,
  );
  url.searchParams.set("X-Amz-Expires", String(expiresIn));

  const signed = await client.sign(
    new Request(url, { method: "PUT", headers: { "Content-Type": contentType } }),
    { aws: { signQuery: true } },
  );

  return signed.url.toString();
}
