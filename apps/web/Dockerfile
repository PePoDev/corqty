FROM node:24-alpine AS dependencies
RUN corepack enable
WORKDIR /workspace
ENV NEXT_TELEMETRY_DISABLED=1
COPY package.json pnpm-workspace.yaml pnpm-lock.yaml ./
COPY apps/web/package.json apps/web/package.json
RUN pnpm install --frozen-lockfile

FROM dependencies AS build
# Next rewrites are resolved at build time. Compose passes this argument;
# deployments using another internal API address must build with it as well.
ARG CORQTY_API_ORIGIN=http://api:8080
ENV CORQTY_API_ORIGIN=$CORQTY_API_ORIGIN
COPY apps/web apps/web
RUN pnpm web:build

FROM node:24-alpine AS runtime
WORKDIR /workspace
ENV NODE_ENV=production \
    NEXT_TELEMETRY_DISABLED=1 \
    PORT=3000 \
    HOSTNAME=0.0.0.0
COPY --from=build --chown=node:node /workspace/apps/web/.next/standalone ./
COPY --from=build --chown=node:node /workspace/apps/web/.next/static ./apps/web/.next/static
USER node
EXPOSE 3000
CMD ["node", "apps/web/server.js"]
