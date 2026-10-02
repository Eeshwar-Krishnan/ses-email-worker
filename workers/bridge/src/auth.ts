const encoder = new TextEncoder();

let cachedKey: CryptoKey | null = null;
let cachedSecret: string | null = null;

async function hmacKey(secret: string): Promise<CryptoKey> {
  if (cachedKey && cachedSecret === secret) {
    return cachedKey;
  }
  cachedKey = await crypto.subtle.importKey(
    "raw",
    encoder.encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign", "verify"],
  );
  cachedSecret = secret;
  return cachedKey;
}

export interface VerifiedRequest {
  body: Uint8Array;
}

/**
 * Verifies the Lambda's HMAC over `method\npath\ntimestamp\nnonce\nsha256(body)`.
 *
 * Returns the raw body on success so callers can parse it without reading the
 * request stream twice.
 */
export async function verifyRequest(
  request: Request,
  secret: string,
  allowedSkewSeconds: number,
): Promise<VerifiedRequest | Response> {
  const timestamp = request.headers.get("x-timestamp");
  const nonce = request.headers.get("x-nonce");
  const signature = request.headers.get("x-signature");

  if (!timestamp || !nonce || !signature) {
    return new Response("missing signature headers", { status: 401 });
  }

  const asserted = Number(timestamp);
  const now = Math.floor(Date.now() / 1000);
  if (!Number.isFinite(asserted) || Math.abs(now - asserted) > allowedSkewSeconds) {
    return new Response("timestamp outside allowed window", { status: 401 });
  }

  const signatureBytes = fromHex(signature);
  if (!signatureBytes) {
    return new Response("malformed signature", { status: 401 });
  }

  const body = new Uint8Array(await request.arrayBuffer());
  const bodyHash = hex(await crypto.subtle.digest("SHA-256", body));
  const path = new URL(request.url).pathname;
  const canonical = `${request.method}\n${path}\n${timestamp}\n${nonce}\n${bodyHash}`;

  const key = await hmacKey(secret);
  const valid = await crypto.subtle.verify(
    "HMAC",
    key,
    signatureBytes,
    encoder.encode(canonical),
  );

  if (!valid) {
    return new Response("invalid signature", { status: 401 });
  }

  return { body };
}

export function isResponse(value: VerifiedRequest | Response): value is Response {
  return value instanceof Response;
}

function hex(buffer: ArrayBuffer): string {
  return [...new Uint8Array(buffer)]
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}

function fromHex(value: string): Uint8Array | null {
  if (value.length === 0 || value.length % 2 !== 0 || /[^0-9a-fA-F]/.test(value)) {
    return null;
  }
  const bytes = new Uint8Array(value.length / 2);
  for (let i = 0; i < bytes.length; i += 1) {
    bytes[i] = Number.parseInt(value.slice(i * 2, i * 2 + 2), 16);
  }
  return bytes;
}
