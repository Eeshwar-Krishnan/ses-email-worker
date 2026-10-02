import { isResponse, verifyRequest } from "./auth";
import { handleEmail } from "./email";
import { presignPut } from "./r2";
import type { EmailMetadata, Env } from "./types";

const DEFAULT_SKEW_SECONDS = 300;
const DEFAULT_PRESIGN_TTL_SECONDS = 3600;
const KEY_PATTERN = /^[A-Za-z0-9._/-]+$/;

export default {
  async fetch(request, env): Promise<Response> {
    const { pathname } = new URL(request.url);

    if (request.method !== "POST") {
      return new Response("method not allowed", { status: 405 });
    }

    switch (pathname) {
      case "/presign":
        return handlePresign(request, env);
      case "/enqueue":
        return handleEnqueue(request, env);
      default:
        return new Response("not found", { status: 404 });
    }
  },

  async queue(batch, env): Promise<void> {
    for (const message of batch.messages) {
      try {
        await handleEmail(env, message.body);
        message.ack();
      } catch (error) {
        console.error("failed to process email", message.id, error);
        message.retry();
      }
    }
  },
} satisfies ExportedHandler<Env, EmailMetadata>;

async function handlePresign(request: Request, env: Env): Promise<Response> {
  const verified = await verifyRequest(request, env.BRIDGE_HMAC_SECRET, skew(env));
  if (isResponse(verified)) {
    return verified;
  }

  const payload = parseJson<{ key?: string; contentType?: string; expiresIn?: number }>(
    verified.body,
  );
  if (!payload?.key) {
    return new Response("missing key", { status: 400 });
  }

  const prefix = env.R2_KEY_PREFIX ?? "inbound";
  if (!KEY_PATTERN.test(payload.key) || !payload.key.startsWith(`${prefix}/`)) {
    return new Response("key outside allowed prefix", { status: 400 });
  }

  const maxTtl = numberVar(env.PRESIGN_TTL_SECONDS, DEFAULT_PRESIGN_TTL_SECONDS);
  const ttl =
    payload.expiresIn && payload.expiresIn > 0
      ? Math.min(payload.expiresIn, maxTtl)
      : maxTtl;
  const url = await presignPut(env, payload.key, payload.contentType ?? "application/octet-stream", ttl);
  return Response.json({ url });
}

async function handleEnqueue(request: Request, env: Env): Promise<Response> {
  const verified = await verifyRequest(request, env.BRIDGE_HMAC_SECRET, skew(env));
  if (isResponse(verified)) {
    return verified;
  }

  const metadata = parseJson<EmailMetadata>(verified.body);
  if (!metadata?.messageId || !metadata?.r2Key) {
    return new Response("missing required metadata", { status: 400 });
  }

  await env.EMAIL_QUEUE.send(metadata);
  return Response.json({ ok: true });
}

function parseJson<T>(body: Uint8Array): T | null {
  try {
    return JSON.parse(new TextDecoder().decode(body)) as T;
  } catch {
    return null;
  }
}

function skew(env: Env): number {
  return numberVar(env.ALLOWED_SKEW_SECONDS, DEFAULT_SKEW_SECONDS);
}

function numberVar(value: string | undefined, fallback: number): number {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
}
