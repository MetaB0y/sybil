import createClient from "openapi-fetch";
import type { paths } from "./schema";

const baseUrl = process.env.NEXT_PUBLIC_API_BASE;
if (!baseUrl) {
  throw new Error("NEXT_PUBLIC_API_BASE is not set. Copy .env.example to .env.local.");
}

export const api = createClient<paths>({ baseUrl });
