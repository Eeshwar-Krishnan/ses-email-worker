import type { EmailMetadata, Env } from "./types";

/**
 * Queue consumer entry point: loads the raw MIME message from R2 and hands it
 * to `processEmail`.
 */
export async function handleEmail(env: Env, meta: EmailMetadata): Promise<void> {
  const object = await env.EMAILS.get(meta.r2Key);
  if (!object) {
    console.warn("r2 object missing for queued email", meta.r2Key);
    return;
  }

  const raw = await object.arrayBuffer();
  await processEmail(env, meta, raw);
}

/**
 * Extension point for your email worker logic.
 *
 * The raw message is the original SES MIME bytes and `meta.recipients` comes
 * from the SMTP delivery headers, which is what you want for wildcard routing
 * (rather than trusting `To:` alone).
 */
export async function processEmail(
  _env: Env,
  meta: EmailMetadata,
  raw: ArrayBuffer,
): Promise<void> {
  console.log(
    JSON.stringify({
      event: "email.received",
      messageId: meta.messageId,
      recipients: meta.recipients,
      from: meta.from,
      subject: meta.subject,
      bytes: raw.byteLength,
    }),
  );
}
