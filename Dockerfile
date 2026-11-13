FROM node:24.22.1-bookworm-slim AS builder

WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY src ./src
COPY tsconfig.json ./
RUN npm run build

FROM node:24.22.1-bookworm-slim AS runtime-deps

WORKDIR /app
COPY package.json package-lock.json ./
# The bundle externalizes only these native modules.
RUN node -e "const fs = require('node:fs'); const names = ['@maplibre/native-ffi', '@maplibre/native-ffi-opengl', 'sharp']; const p = require('./package.json'); const lock = require('./package-lock.json'); p.dependencies = Object.fromEntries(names.map((name) => [name, lock.packages['node_modules/' + name].version])); delete p.devDependencies; fs.writeFileSync('package.json', JSON.stringify(p)); fs.unlinkSync('package-lock.json');" \
  && npm install --omit=dev --no-package-lock --no-audit --no-fund

# MapLibre Native renders offscreen through EGL's surfaceless platform; Mesa's
# llvmpipe rasterizes on the CPU. No X server or display is needed.
FROM ubuntu:noble AS gl-base
ARG DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
  libegl1 \
  libegl-mesa0 \
  libgles2 \
  libgl1-mesa-dri \
  && rm -rf /var/lib/apt/lists/*

# Dev image for docker-compose: full Node toolchain (npm included) with all
# dependencies installed at build time, so the container is ready at startup.
# src is provided by a bind mount.
FROM gl-base AS dev

COPY --from=node:24.22.1-bookworm-slim /usr/local/bin /usr/local/bin
COPY --from=node:24.22.1-bookworm-slim /usr/local/lib/node_modules /usr/local/lib/node_modules

WORKDIR /app
COPY package.json package-lock.json tsconfig.json ./
RUN npm ci --no-audit --no-fund

FROM gl-base AS runtime

COPY --from=public.ecr.aws/awsguru/aws-lambda-adapter:1.0.1 /lambda-adapter /opt/extensions/lambda-adapter
ENV AWS_LWA_PORT=3000
ENV AWS_LWA_READINESS_CHECK_PATH=/health
# If prewarm pushes INIT past Lambda's 10s limit, continue it during the
# first invoke instead of letting Lambda restart the sandbox.
ENV AWS_LWA_ASYNC_INIT=true

# npm and Corepack are unnecessary at runtime.
COPY --from=node:24.22.1-bookworm-slim /usr/local/bin/node /usr/local/bin/node

WORKDIR /app
COPY --from=builder /app/build ./build
COPY --from=runtime-deps /app/node_modules ./node_modules

CMD ["node", "/app/build/main.cjs", "tile-server"]
