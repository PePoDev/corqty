import path from "node:path"
import type { NextConfig } from "next"

const apiOrigin = process.env.CORQTY_API_ORIGIN ?? "http://127.0.0.1:8080"

const nextConfig: NextConfig = {
  agentRules: false,
  turbopack: { root: path.resolve(import.meta.dirname, "../..") },
  async rewrites() {
    return [
      { source: "/api/v1/:path*", destination: `${apiOrigin}/api/v1/:path*` },
    ]
  },
}

export default nextConfig
