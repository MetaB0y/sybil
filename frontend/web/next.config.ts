import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  // Emit a self-contained server bundle (.next/standalone) so the Docker image
  // can run `node server.js` without pnpm/node_modules. See frontend/web/Dockerfile.
  output: "standalone",
};

export default nextConfig;
