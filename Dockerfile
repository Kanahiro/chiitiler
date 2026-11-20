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

# Runtime dependencies the bundle leaves external: the renderer addon (@ml2drender/node), and
# @napi-rs/image for lossy WebP. --prod leaves out that optional peer, so npm installs the locked
# version on its own (with only this platform's addon).
FROM builder AS deps
RUN v=$(node -p "require('./node_modules/@napi-rs/image/package.json').version") \
    && rm -rf node_modules && pnpm install --frozen-lockfile --prod \
    && npm install --prefix /opt/napi-image --no-save --no-package-lock --no-audit --no-fund "@napi-rs/image@$v"

# Distroless: no shell, npm or apt. Entrypoint is /nodejs/bin/node (Node 24.22.3 at this digest).
FROM gcr.io/distroless/nodejs24-debian13@sha256:24bc0fe0225c7518f202a822587dad74fe143b96326b81fe5cc08c1df8bed03e AS runtime

WORKDIR /app
COPY --from=builder /app/build ./build
COPY --from=deps /app/node_modules ./node_modules
COPY --from=deps /opt/napi-image/node_modules/@napi-rs ./node_modules/@napi-rs
COPY crs ./crs
ENV CHIITILER_CRS_DIR=/app/crs

CMD ["/app/build/main.mjs"]
