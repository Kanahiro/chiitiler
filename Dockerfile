FROM rust:1.97-slim-bookworm AS native

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY .cargo ./.cargo
COPY crates ./crates
# The Node-API renderer for this image's platform, named like scripts/build-native.ts names it.
RUN cargo build -p ml2drender-napi --profile native \
  && mkdir -p native \
  && arch=$(uname -m | sed 's/x86_64/x64/; s/aarch64/arm64/') \
  && cp target/native/libml2drender_napi.so "native/ml2drender.linux-$arch.node"

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
COPY --from=native /app/native ./native

FROM node:24.22.3-bookworm-slim AS runtime

COPY --from=public.ecr.aws/awsguru/aws-lambda-adapter:1.0.1 /lambda-adapter /opt/extensions/lambda-adapter
ENV AWS_LWA_PORT=3000
ENV AWS_LWA_READINESS_CHECK_PATH=/health
# If prewarm pushes INIT past Lambda's 10s limit, continue it during the
# first invoke instead of letting Lambda restart the sandbox.
ENV AWS_LWA_ASYNC_INIT=true

WORKDIR /app
# The bundle includes every dependency but the renderer, which it loads from ../native.
COPY --from=builder /app/build ./build
COPY --from=native /app/native ./native

CMD ["node", "/app/build/main.mjs", "tile-server"]
