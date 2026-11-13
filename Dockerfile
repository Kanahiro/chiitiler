FROM rust:1.97-slim-bookworm AS wasm

WORKDIR /app
RUN rustup target add wasm32-unknown-unknown
COPY Cargo.toml Cargo.lock ./
COPY .cargo ./.cargo
COPY crates ./crates
RUN cargo build -p ml2drender-wasm --target wasm32-unknown-unknown --profile wasm \
  && mkdir -p wasm \
  && cp target/wasm32-unknown-unknown/wasm/ml2drender_wasm.wasm wasm/

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
COPY --from=wasm /app/wasm ./wasm

FROM node:24.22.3-bookworm-slim AS runtime

COPY --from=public.ecr.aws/awsguru/aws-lambda-adapter:1.0.1 /lambda-adapter /opt/extensions/lambda-adapter
ENV AWS_LWA_PORT=3000
ENV AWS_LWA_READINESS_CHECK_PATH=/health
# If prewarm pushes INIT past Lambda's 10s limit, continue it during the
# first invoke instead of letting Lambda restart the sandbox.
ENV AWS_LWA_ASYNC_INIT=true

WORKDIR /app
# The bundle includes every dependency; it reads the WASM from ../wasm.
COPY --from=builder /app/build ./build
COPY --from=wasm /app/wasm ./wasm

CMD ["node", "/app/build/main.mjs", "tile-server"]
