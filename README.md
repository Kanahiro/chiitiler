# chiitiler - Tiny VectorTile rendering server

chii-tiler

"tiny" in Japanese is "chiisai", shorten this into "chii"

## motivation

-   In this type of server, there is a de-facto - [maptiler/tileserver-gl](https://github.com/maptiler/tileserver-gl), but this is too big for me.
-   I want a server accept style.json-url and respond raster tile, inspired by [developmentseed/titiler](https://github.com/developmentseed/titiler)

## status

-   this project is under development and experiment

## usage

### Local

-   Node.js v18

```sh
npm install
npm run build
node dist/main.js tile-server
# running server: http://localhost:3000

# develop
npm start # tsc && node dist/main.js tiler-server -D
# running server: http://localhost:3000
# debug page: http://localhost:3000/debug
```

#### options

```sh
node dist/main.js tile-server -p 8000 -c file -fcd cachedir -D
# -p: port number
# -c: cache method
# -fcd: cache directory
# -D: debug mode

node dist/main.js tile-server -c s3 -s3b chiitiler -s3r ap-northeast-1
# -s3b: S3 bucket name for cache
# -s3r: S3 bucket region
```

#### debug page

-   in debug mode, you can access debug page: http://localhost:3000/debug
-   You can pass style.json url:
    -   http://localhost:3000/debug?url=https://tile.openstreetmap.jp/styles/osm-bright/style.json

### Container Image

```sh
docker pull ghcr.io/kanahiro/chiitiler
docker run -p 3000:3000 ghcr.io/kanahiro/chiitiler # -> chiitiler tile-server
```

#### Environment Variables

you can pass server options via environment variables

| env var                        | default  | description           |
| ------------------------------ | -------- | --------------------- |
| CHIITILER_PORT                 | 3000     | port number           |
| CHIITILER_DEBUG                | false    | debug mode            |
| CHIITILER_CACHE_METHOD         | none     | cache method          |
| CHIITILER_CACHE_FILECACHE_DIR  | .cache   | filecache directory   |
| CHIITILER_CACHE_S3CACHE_BUCKET |          | s3cache bucket name   |
| CHIITILER_CACHE_S3CACHE_REGION | us-east1 | s3cache bucket region |
