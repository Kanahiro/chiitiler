FROM ubuntu:20.04

# Install dependencies
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update
RUN apt-get install -y \
  ccache \
  cmake \
  ninja-build \
  pkg-config \
  xvfb \
  libcurl4-openssl-dev \
  libglfw3-dev \
  libuv1-dev \
  g++-10 \
  libc++-9-dev \
  libc++abi-9-dev \
  libjpeg-dev \
  libpng-dev \
  libwebp-dev
RUN /usr/sbin/update-ccache-symlinks

# Install Node.js
RUN apt-get install -y ca-certificates curl gnupg
RUN mkdir -p /etc/apt/keyrings
RUN curl -fsSL https://deb.nodesource.com/gpgkey/nodesource-repo.gpg.key | gpg --dearmor -o /etc/apt/keyrings/nodesource.gpg
RUN echo "deb [signed-by=/etc/apt/keyrings/nodesource.gpg] https://deb.nodesource.com/node_18.x nodistro main" | tee /etc/apt/sources.list.d/nodesource.list
RUN apt-get update
RUN apt-get install nodejs -y

# install nodejs dependencies
WORKDIR /app/
COPY package*.json /app/
RUN npm install
COPY . .

# Lambda WebAdapter
COPY --from=public.ecr.aws/awsguru/aws-lambda-adapter:0.7.1 /lambda-adapter /opt/extensions/lambda-adapter
ENV PORT=3000
ENV READINESS_CHECK_PATH=/health

# start server
ENTRYPOINT ["/bin/sh", "-c", "/usr/bin/xvfb-run -a $@", ""]
CMD ["npm", "start"]
