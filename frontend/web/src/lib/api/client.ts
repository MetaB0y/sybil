import createClient from "openapi-fetch";
import {
  clearStoredAccount,
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
    if (
      request.method === "GET" &&
      response.status === 401 &&
      request.headers.get("authorization")?.startsWith("Bearer sybk_")
    ) {
      clearStoredAccount();
      globalThis.dispatchEvent?.(new Event(READ_AUTH_INVALID_EVENT));
    }
  },
});
