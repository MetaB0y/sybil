import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  // Emit a self-contained server bundle (.next/standalone) so the Docker image
  // can run `node server.js` without pnpm/node_modules. See frontend/web/Dockerfile.
  output: "standalone",
  images: {
    // Mirrored market artwork comes from this shared Polymarket bucket. Keep
    // the optimizer allowlist deliberately narrow: image URLs are upstream
    // metadata, so a broad remote pattern would turn our server into an open
    // image proxy. Thumbnails only need one low-cost quality tier.
    remotePatterns: [
      {
        protocol: "https",
        hostname: "polymarket-upload.s3.us-east-2.amazonaws.com",
        port: "",
        pathname: "/**",
        search: "",
      },
    ],
    qualities: [60],
  },
};

export default nextConfig;
