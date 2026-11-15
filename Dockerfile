FROM node:24.22.3-bookworm-slim AS builder

WORKDIR /app
RUN corepack enable pnpm
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY src ./src
COPY tsconfig.json ./
RUN pnpm build

# Dev image for docker-compose: dependencies installed at build time, src from a bind mount.
FROM builder AS dev

# Runtime dependencies the bundle leaves external: the renderer addon (@ml2drender/node).
FROM builder AS deps
RUN rm -rf node_modules && pnpm install --frozen-lockfile --prod

FROM node:24.22.3-bookworm-slim AS runtime

COPY --from=public.ecr.aws/awsguru/aws-lambda-adapter:1.0.1 /lambda-adapter /opt/extensions/lambda-adapter
ENV AWS_LWA_PORT=3000
ENV AWS_LWA_READINESS_CHECK_PATH=/health

WORKDIR /app
COPY --from=builder /app/build ./build
COPY --from=deps /app/node_modules ./node_modules

CMD ["node", "/app/build/main.mjs"]
