FROM node:24-bookworm-slim AS builder

WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY src ./src
COPY tsconfig.json ./
RUN npm run build

FROM node:24-bookworm-slim AS runtime-deps

WORKDIR /app
COPY package.json package-lock.json ./
# The bundle externalizes only these native modules. maplibre's remaining
# JavaScript dependencies are install tooling, not runtime dependencies.
RUN node -e "const fs = require('node:fs'); const names = ['@maplibre/maplibre-gl-native', 'sharp']; const p = require('./package.json'); const lock = require('./package-lock.json'); p.dependencies = Object.fromEntries(names.map((name) => [name, lock.packages['node_modules/' + name].version])); delete p.devDependencies; fs.writeFileSync('package.json', JSON.stringify(p)); fs.unlinkSync('package-lock.json');" \
  && npm install --omit=dev --no-package-lock --no-audit --no-fund \
  && mv node_modules/@maplibre/maplibre-gl-native /tmp/maplibre-gl-native \
  && node -e "const fs = require('node:fs'); const p = require('./package.json'); delete p.dependencies['@maplibre/maplibre-gl-native']; fs.writeFileSync('package.json', JSON.stringify(p));" \
  && npm prune --omit=dev --no-audit --no-fund \
  && mkdir -p node_modules/@maplibre \
  && mv /tmp/maplibre-gl-native node_modules/@maplibre/maplibre-gl-native

FROM ubuntu:noble AS runtime

# mbgl.node requires GLIBC_2.38 and GLIBCXX_3.4.32. These `ldd`-derived
# libraries and Xvfb provide its headless GL runtime on Ubuntu Noble.
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

COPY --from=public.ecr.aws/awsguru/aws-lambda-adapter:1.0.0 /lambda-adapter /opt/extensions/lambda-adapter
ENV PORT=3000
ENV AWS_LWA_READINESS_CHECK_PATH=/health

# npm and Corepack are unnecessary at runtime.
COPY --from=node:24-bookworm-slim /usr/local/bin/node /usr/local/bin/node

WORKDIR /app
COPY --from=builder /app/build ./build
COPY --from=runtime-deps /app/node_modules ./node_modules

COPY --chmod=755 docker-entrypoint.sh ./docker-entrypoint.sh
ENTRYPOINT [ "/app/docker-entrypoint.sh" ]

CMD ["node", "/app/build/main.cjs", "tile-server"]
