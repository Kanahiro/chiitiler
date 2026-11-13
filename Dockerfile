FROM node:24.22.3-bookworm-slim AS builder

WORKDIR /app
RUN corepack enable pnpm
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY src ./src
COPY tsconfig.json ./
RUN pnpm build

FROM node:24.22.3-bookworm-slim AS runtime-deps

WORKDIR /app
RUN corepack enable pnpm
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
# The bundle externalizes only these native modules. maplibre's remaining
# JavaScript dependencies are install tooling, not runtime dependencies.
# Versions stay pinned by pnpm-lock.yaml; the hoisted linker gives a flat node_modules.
RUN node -e "const fs = require('node:fs'); const p = require('./package.json'); p.dependencies = { '@maplibre/maplibre-gl-native': p.dependencies['@maplibre/maplibre-gl-native'], sharp: p.dependencies.sharp }; delete p.devDependencies; fs.writeFileSync('package.json', JSON.stringify(p));" \
  && pnpm install --prod --config.node-linker=hoisted \
  && mv node_modules/@maplibre/maplibre-gl-native /tmp/maplibre-gl-native \
  && node -e "const fs = require('node:fs'); const p = require('./package.json'); delete p.dependencies['@maplibre/maplibre-gl-native']; fs.writeFileSync('package.json', JSON.stringify(p));" \
  && pnpm install --prod --config.node-linker=hoisted \
  && mkdir -p node_modules/@maplibre \
  && mv /tmp/maplibre-gl-native node_modules/@maplibre/maplibre-gl-native

# mbgl.node requires GLIBC_2.38 and GLIBCXX_3.4.32. These `ldd`-derived
# libraries and Xvfb provide its headless GL runtime on Ubuntu Noble.
FROM ubuntu:noble AS gl-base
ARG DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
  xvfb \
  libopengl0 \
  libglx0 \
  libcurl4t64 \
  libjpeg-turbo8 \
  libuv1t64 \
  libx11-6 \
  libxext6 \
  libwebp7 \
  libicu74 \
  libpng16-16t64 \
  && rm -rf /var/lib/apt/lists/*

# Dev image for docker-compose: full Node toolchain (corepack included) with all
# dependencies installed at build time, so the container is ready at startup.
# src is provided by a bind mount.
FROM gl-base AS dev

COPY --from=node:24.22.3-bookworm-slim /usr/local/bin /usr/local/bin
COPY --from=node:24.22.3-bookworm-slim /usr/local/lib/node_modules /usr/local/lib/node_modules

WORKDIR /app
RUN corepack enable pnpm
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml tsconfig.json ./
RUN pnpm install --frozen-lockfile

FROM gl-base AS runtime

COPY --from=public.ecr.aws/awsguru/aws-lambda-adapter:1.0.1 /lambda-adapter /opt/extensions/lambda-adapter
ENV AWS_LWA_PORT=3000
ENV AWS_LWA_READINESS_CHECK_PATH=/health
# If prewarm pushes INIT past Lambda's 10s limit, continue it during the
# first invoke instead of letting Lambda restart the sandbox.
ENV AWS_LWA_ASYNC_INIT=true

# pnpm, npm and Corepack are unnecessary at runtime.
COPY --from=node:24.22.3-bookworm-slim /usr/local/bin/node /usr/local/bin/node

WORKDIR /app
COPY --from=builder /app/build ./build
COPY --from=runtime-deps /app/node_modules ./node_modules

COPY --chmod=755 docker-entrypoint.sh ./docker-entrypoint.sh
ENTRYPOINT [ "/app/docker-entrypoint.sh" ]

CMD ["node", "/app/build/main.cjs", "tile-server"]
