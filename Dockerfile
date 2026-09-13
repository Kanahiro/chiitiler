# Build the same MapLibre release as package-lock.json, for this image's Node ABI.
# EGL is already supported upstream; no fork or source patch is needed.
FROM ubuntu:noble AS native-builder
ARG DEBIAN_FRONTEND=noninteractive
ARG NATIVE_BUILD_JOBS=2
RUN apt-get update && apt-get install -y --no-install-recommends \
  ca-certificates git clang lld cmake ninja-build ccache pkg-config \
  libcurl4-openssl-dev libuv1-dev libpng-dev libicu-dev libjpeg-turbo8-dev \
  libwebp-dev libegl1-mesa-dev libgles2-mesa-dev \
  && rm -rf /var/lib/apt/lists/*
COPY --from=node:24-bookworm-slim /usr/local/bin/node /usr/local/bin/node
WORKDIR /native
COPY package-lock.json ./
RUN version=$(node -p "require('./package-lock.json').packages['node_modules/@maplibre/maplibre-gl-native'].version") \
  && git clone --branch "node-v${version}" --depth 1 --recurse-submodules --shallow-submodules \
    https://github.com/maplibre/maplibre-native.git source
RUN cmake --preset linux-opengl-node -S source -B build \
    -DCMAKE_BUILD_TYPE=Release \
    -DMLN_WITH_EGL=ON -DMLN_WITH_X11=OFF -DMLN_WITH_WAYLAND=OFF -DMLN_WITH_GLFW=OFF \
    -DMLN_WITH_WERROR=OFF -DCMAKE_SHARED_LINKER_FLAGS=-fuse-ld=lld \
  && cmake --build build --target "mbgl-node.abi-$(node -p process.versions.modules)" --parallel "$NATIVE_BUILD_JOBS"

FROM node:24-bookworm-slim AS builder

WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci --ignore-scripts --no-audit --no-fund
COPY src ./src
COPY tsconfig.json ./
RUN npm run build

FROM node:24-bookworm-slim AS runtime-deps

WORKDIR /app
COPY package.json package-lock.json ./
# The bundle externalizes only these native modules. maplibre's remaining
# JavaScript dependencies are install tooling, not runtime dependencies.
RUN node -e "const fs = require('node:fs'); const names = ['@maplibre/maplibre-gl-native', 'sharp']; const p = require('./package.json'); const lock = require('./package-lock.json'); p.dependencies = Object.fromEntries(names.map((name) => [name, lock.packages['node_modules/' + name].version])); delete p.devDependencies; fs.writeFileSync('package.json', JSON.stringify(p)); fs.unlinkSync('package-lock.json');" \
  && npm install --ignore-scripts --omit=dev --no-package-lock --no-audit --no-fund \
  && mv node_modules/@maplibre/maplibre-gl-native /tmp/maplibre-gl-native \
  && node -e "const fs = require('node:fs'); const p = require('./package.json'); delete p.dependencies['@maplibre/maplibre-gl-native']; fs.writeFileSync('package.json', JSON.stringify(p));" \
  && npm prune --ignore-scripts --omit=dev --no-audit --no-fund \
  && mkdir -p node_modules/@maplibre \
  && mv /tmp/maplibre-gl-native node_modules/@maplibre/maplibre-gl-native

# Keep the native build and runtime on the same libc / libstdc++ distribution.
# Mesa provides CPU rendering through EGL without a display server.
FROM ubuntu:noble AS gl-base
ARG DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
  libegl1 \
  libgl1-mesa-dri \
  libopengl0 \
  libcurl4t64 \
  libjpeg-turbo8 \
  libuv1t64 \
  libwebp7 \
  libicu74 \
  libpng16-16t64 \
  && rm -rf /var/lib/apt/lists/*

ENV EGL_PLATFORM=surfaceless
ENV LIBGL_ALWAYS_SOFTWARE=true

# Dev image for docker-compose: full Node toolchain (npm included) with all
# dependencies installed at build time, so the container is ready at startup.
# src is provided by a bind mount.
FROM gl-base AS dev

COPY --from=node:24-bookworm-slim /usr/local/bin /usr/local/bin
COPY --from=node:24-bookworm-slim /usr/local/lib/node_modules /usr/local/lib/node_modules

WORKDIR /app
COPY package.json package-lock.json tsconfig.json ./
RUN npm ci --ignore-scripts --no-audit --no-fund
COPY --from=native-builder /native/source/platform/node/lib/ ./node_modules/@maplibre/maplibre-gl-native/lib/

FROM gl-base AS runtime

COPY --from=public.ecr.aws/awsguru/aws-lambda-adapter:1.0.1 /lambda-adapter /opt/extensions/lambda-adapter
ENV AWS_LWA_PORT=3000
ENV AWS_LWA_READINESS_CHECK_PATH=/health
# If prewarm pushes INIT past Lambda's 10s limit, continue it during the
# first invoke instead of letting Lambda restart the sandbox.
ENV AWS_LWA_ASYNC_INIT=true

# npm and Corepack are unnecessary at runtime.
COPY --from=node:24-bookworm-slim /usr/local/bin/node /usr/local/bin/node

WORKDIR /app
COPY --from=builder /app/build ./build
COPY --from=runtime-deps /app/node_modules ./node_modules
COPY --from=native-builder /native/source/platform/node/lib/ ./node_modules/@maplibre/maplibre-gl-native/lib/

CMD ["node", "/app/build/main.cjs", "tile-server"]
