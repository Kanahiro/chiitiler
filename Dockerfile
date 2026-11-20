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

# Only what the bundle leaves external: the renderer addon (@ml2drender/node) and @napi-rs/image
# (lossy WebP), at the locked versions, with only this platform's addon. Everything else is bundled,
# so peers (e.g. the 11MB style spec) are omitted.
FROM builder AS deps
RUN v() { node -p "require('./node_modules/$1/package.json').version"; } \
    && npm install --prefix /opt/deps --no-save --no-package-lock --no-audit --no-fund --omit=peer \
       "@ml2drender/node@$(v @ml2drender/node)" "@napi-rs/image@$(v @napi-rs/image)"

# Distroless: no shell, npm or apt. Entrypoint is /nodejs/bin/node (Node 24.22.3 at this digest).
FROM gcr.io/distroless/nodejs24-debian13@sha256:24bc0fe0225c7518f202a822587dad74fe143b96326b81fe5cc08c1df8bed03e AS runtime

WORKDIR /app
COPY --from=builder /app/build ./build
COPY --from=deps /opt/deps/node_modules ./node_modules
COPY crs ./crs
ENV CHIITILER_CRS_DIR=/app/crs

CMD ["/app/build/main.mjs"]
