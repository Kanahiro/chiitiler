# chiitiler - Tiny VectorTile rendering server

chii-tiler

"tiny" in Japanese is "chiisai", shorten this into "chii"

## motivation

- In this type of server, there is a de-facto - [maptiler/tileserver-gl](https://github.com/maptiler/tileserver-gl), but this is too big for me.
- I want a server accept style.json-url and respond raster tile, inspired by [developmentseed/titiler](https://github.com/developmentseed/titiler)

## status

- this project is under development and experiment

## usage

```sh
npm install
npm run build
npm start

# then server will start
# http://localhost:3000/debug
```

- You can pass style.json url:
    - http://localhost:3000/debug?url=https://tile.openstreetmap.jp/styles/osm-bright/style.json