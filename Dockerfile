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

# Runtime dependencies the bundle leaves external: the renderer addon (@ml2drender/node).
FROM builder AS deps
RUN rm -rf node_modules && pnpm install --frozen-lockfile --prod

# Preset CRSs (EPSG:4326 and EPSG's projected CRSs) from PROJ's database, for ?crs=EPSG:<code>.
FROM python:3.13-slim-bookworm AS crs-generate
RUN pip install --no-cache-dir pyproj==3.8.0
COPY crs/generate.py /generate.py
RUN python -I /generate.py /crs

# ... only those the renderer supports.
FROM deps AS crs
COPY --from=crs-generate /crs /crs
COPY crs/validate.mjs ./validate.mjs
RUN node validate.mjs /crs

FROM node:24.22.3-bookworm-slim AS runtime

COPY --from=public.ecr.aws/awsguru/aws-lambda-adapter:1.0.1 /lambda-adapter /opt/extensions/lambda-adapter
ENV AWS_LWA_PORT=3000
ENV AWS_LWA_READINESS_CHECK_PATH=/health

WORKDIR /app
COPY --from=builder /app/build ./build
COPY --from=deps /app/node_modules ./node_modules
COPY --from=crs /crs ./crs
ENV CHIITILER_CRS_DIR=/app/crs

CMD ["node", "/app/build/main.mjs"]
