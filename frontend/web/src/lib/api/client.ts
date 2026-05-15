import createClient from "openapi-fetch";
import type { paths } from "./schema";

const DEFAULT_API_BASE = "https://172-104-31-54.nip.io";
const baseUrl = process.env.NEXT_PUBLIC_API_BASE ?? DEFAULT_API_BASE;

export const api = createClient<paths>({ baseUrl });
