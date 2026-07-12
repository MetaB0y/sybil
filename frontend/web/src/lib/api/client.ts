import createClient from "openapi-fetch";
import {
  clearStoredReadApiKey,
  readStoredReadApiKey,
} from "@/lib/account/storage";
import type { paths } from "./schema";

const DEFAULT_API_BASE = "https://172-104-31-54.nip.io";
const baseUrl = process.env.NEXT_PUBLIC_API_BASE ?? DEFAULT_API_BASE;

export const api = createClient<paths>({ baseUrl });

export const READ_AUTH_INVALID_EVENT = "sybil:read-auth-invalid";

api.use({
  onRequest({ request }) {
    if (request.method !== "GET" || request.headers.has("authorization")) {
      return;
    }
    const token = readStoredReadApiKey();
    if (token) request.headers.set("authorization", `Bearer ${token}`);
  },
  onResponse({ request, response }) {
    // A revoked/invalid read key is a 401. A 403 means the key is valid but the
    // caller asked for another account, so it must not destroy the session.
    const authorization = request.headers.get("authorization");
    const requestToken = authorization?.startsWith("Bearer sybk_")
      ? authorization.slice("Bearer ".length)
      : null;
    if (
      request.method === "GET" &&
      response.status === 401 &&
      requestToken !== null &&
      readStoredReadApiKey() === requestToken
    ) {
      clearStoredReadApiKey();
      globalThis.dispatchEvent?.(new Event(READ_AUTH_INVALID_EVENT));
    }
  },
});
