FROM node:24-bookworm-slim as builder

WORKDIR /app/
COPY . /app/
RUN npm install
RUN npm run build
# /app/build/main.cjs

FROM ubuntu:noble as runtime

# Install dependencies
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update
RUN apt-get install -y \
  xvfb \
  libcurl4-openssl-dev \
  libglfw3-dev \
  libuv1-dev \
  libjpeg-turbo8-dev \
  libpng-dev \
  libwebp-dev

# Lambda WebAdapter
COPY --from=public.ecr.aws/awsguru/aws-lambda-adapter:0.9.1 /lambda-adapter /opt/extensions/lambda-adapter
ENV PORT=3000
ENV READINESS_CHECK_PATH=/health
ENV AWS_LWA_INVOKE_MODE=response_stream

# Copy Node.js executable from node:24-bookworm-slim
COPY --from=node:24-bookworm-slim /usr/local/bin /usr/local/bin
COPY --from=node:24-bookworm-slim /usr/local/lib/node_modules /usr/local/lib/node_modules

# Copy /app/dist from builder
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