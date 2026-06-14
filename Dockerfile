FROM node:24-bookworm-slim AS builder

WORKDIR /app/
COPY . /app/
RUN npm install
RUN npm run build
# -> /app/build/main.cjs

FROM ubuntu:noble AS runtime

# Runtime shared libraries required by @maplibre/maplibre-gl-native (mbgl.node).
# This is the minimal set derived from `ldd .../mbgl.node` (runtime .so packages only,
# no -dev headers or toolchain), plus xvfb for the headless GL context.
# Why ubuntu:noble and not bookworm-slim: the prebuilt mbgl.node links GLIBC_2.38 /
# GLIBCXX_3.4.32, which Debian bookworm (glibc 2.36) does not provide.
ENV DEBIAN_FRONTEND=noninteractive
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

# Lambda WebAdapter
COPY --from=public.ecr.aws/awsguru/aws-lambda-adapter:1.0.0 /lambda-adapter /opt/extensions/lambda-adapter
ENV PORT=3000
ENV READINESS_CHECK_PATH=/health

# Node.js runtime (taken from bookworm-slim; noble's newer glibc runs it fine).
COPY --from=node:24-bookworm-slim /usr/local/bin /usr/local/bin
COPY --from=node:24-bookworm-slim /usr/local/lib/node_modules /usr/local/lib/node_modules

# Install production dependencies on the runtime image so native modules
# (mbgl.node, sharp) get noble-appropriate prebuilds.
WORKDIR /app/
COPY --from=builder /app/build /app/build
COPY --from=builder /app/package.json /app/package.json
RUN npm install --omit=dev

# Copy entrypoint script
COPY docker-entrypoint.sh /app/docker-entrypoint.sh
RUN chmod +x /app/docker-entrypoint.sh
ENTRYPOINT [ "./docker-entrypoint.sh" ]

# start server
CMD ["node", "/app/build/main.cjs", "tile-server"]
