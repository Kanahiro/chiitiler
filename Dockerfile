FROM node:20-bookworm-slim as builder

WORKDIR /app/
COPY . /app/
RUN npm install
RUN npm run build
# /app/build/main.cjs

FROM public.ecr.aws/ubuntu/ubuntu:22.04 as runtime

# Install dependencies
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update
RUN apt-get upgrade -y
RUN apt-get install -y \
  xvfb \
  libcurl4-openssl-dev \
  libglfw3-dev \
  libuv1-dev \
  libjpeg-dev \
  libpng-dev \
  libwebp-dev

# Lambda WebAdapter
COPY --from=public.ecr.aws/awsguru/aws-lambda-adapter:0.8.3 /lambda-adapter /opt/extensions/lambda-adapter
ENV PORT=3000
ENV READINESS_CHECK_PATH=/health
ENV AWS_LWA_INVOKE_MODE=response_stream

# Copy Node.js executable from node:20-bookworm-slim
COPY --from=node:20-bookworm-slim /usr/local/bin /usr/local/bin
COPY --from=node:20-bookworm-slim /usr/local/lib/node_modules /usr/local/lib/node_modules

# Copy /app/dist from builder
WORKDIR /app/
COPY --from=builder /app/build /app/build
COPY --from=builder /app/package.json /app/package.json
RUN npm install --omit=dev

# start server
ENTRYPOINT ["/bin/sh", "-c", "/usr/bin/xvfb-run -a node /app/build/main.cjs $@", ""]
CMD ["tile-server"]