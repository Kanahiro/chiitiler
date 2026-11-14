<!--
chiitiler に取り込んだ ml2drender の開発ノート(ml2drender リポジトリの AGENTS.md のコピー)。
chiitiler での差分:
- TS ラッパーは `packages/ml2drender/src/index.ts` ではなく `src/render/ml2drender.ts`(`toJpeg` を追加)。WASM は `pnpm build:wasm` で `wasm/` に出力し、`src/wasm.ts` が読み込む。
- `examples/browser`・`pnpm bench`・`pnpm demo` は取り込んでいない。`pnpm test` は chiitiler の vitest なので、Rust のテストは `pnpm test:rust`。
- 採用済みクレートに `jpeg-encoder`(純 Rust、JPEG 書き出し)を追加。
- `render/x4.rs`: WASM SIMD で三角形の行を4画素ずつ処理する経路(円・線(破線・パターン・グラデーションなし)・シンボル・単色の塗り)。ネイティブ(render テスト)はスカラー経路しか通らないので、変更したら render テストのスタイルを変更前後の WASM で描いて RGBA の一致を確かめる。
-->

# MapLibre style 2D タイルレンダラ (Rust / WASM)

## 目的

MapLibre style (style-spec JSON) を**タイルベースの2D表現に限って**仕様準拠でレンダリングする最小のレンダラ。主ターゲットは `wasm32-unknown-unknown`(Cloudflare Workers / Deno / WASI / ブラウザ)。GPU・ネイティブバイナリなしで静止画(RGBA)を返せることが価値。

「完璧」の定義: **対象レイヤについて maplibre-gl-js の render-tests が許容差内で通ること**。ピクセル完全一致は目指さない(GPU vs tiny-skia の差)。

前提を忘れないこと: 正確さだけが目的なら MapLibre Native(ヘッドレス可)を使うほうが安い。本プロジェクトの価値は「GPU不要・軽量・WASM・サーバレス」。

## スコープ

対象:
- style-spec のパース(serde)と式(expression)評価
- ソース: vector(MVT), raster, raster-dem, geojson, image
- 投影: 描画は Web Mercator。カメラは center / zoom / bearing(2D 回転)。投影座標系への再投影(`set_projected_view`: PROJJSON + bbox。PROJ 文字列は受け付けない(CRS の表現は PROJJSON に一本化)。経緯度 / 横メルカトル / メルカトル・Pseudo Mercator / ランベルト正角円錐 / 極ステレオ / laea / Equal Earth / geos、楕円体対応・測地系変換なし、座標は常に東・北の順)は、ベクタ(fill / line / circle / heatmap / symbol / background)は頂点を投影して出力側に直接描く(タイル空間で線分・三角形を細分化、線幅などのピクセル長は投影の局所回転で画面空間に押し出し、タイルのクリップは出力画素ごとの Mercator 位置から決める、投影の反子午線をまたぐタイルは世界コピーで描き分ける。`*-pattern` は投影後の画面ピクセル上に、CRS 座標に固定した原点から整数ズームの Mercator と同じ縮尺(パターン 1px = 1 CSS px、タイルごとのズームに依らない)で一様に敷く。破線と line-pattern は線に沿った投影後の距離で敷き、位相はタイル境界との交点で揃えて交点間を整数周期に伸縮する(タイルごとに切られた線の始点から数えると境界で途切れるため))。raster / DEM だけ Mercator で描いた「絵」をメッシュで warp する。投影ビューのズーム(スタイル評価・ソースタイル選択)は CRS 単位/画素を EPSG:3857 の赤道と同じ式で読む(OGC スケール分母。位置で決めるとタイルごとにズームが変わり境界に継ぎ目が出る)
- レイヤ: background / fill / line / circle / raster / symbol / hillshade / color-relief / heatmap
- sprite(fill-pattern, line-pattern, icon)、glyph(SDF PBF)

非対象(手を出さない): fill-extrusion / 3D / pitch / terrain / globe・GPU 的な非mercator投影(maplibre の globe 等) / sky / light、実行時のスタイル・カメラ変更(setPaintProperty / setStyle / setFeatureState 等。render テストの `operations` は wait 系以外を含めば skip)、ネットワーク取得・キャッシュ・PMTiles 読み出し(ホストの責務)。

## 設計制約(厳守)

- **純Rustのみ。C依存禁止**(zstd, freetype, harfbuzz, GDAL 等は不可)。依存クレートは採用前に `cargo build -p ml2drender --target wasm32-unknown-unknown` で確認する。
- **API の最小形は「style.json + XYZ → タイル画像」**: `Renderer::new(style, View::tile(z, x, y, tile_size))`。`View`(center/zoom/bearing/サイズ/pixel_ratio)は任意カメラ用の一般形で、render テストはこちらを使う。スタイルは生成時に固定(変更 API は作らない)。`set_view` でカメラだけ切り替えると、取得済みリソースとタイル単位のバケット(線のテッセレーション・塗りの earcut)を再利用する(ソースごとに LRU で `MAX_CACHED_TILES` まで)。再利用しないホストは `set_keep_buckets(false)`(TS 側は `ttlMs: 0`)で、線・塗りのバケットを最後に使うレイヤを描いた時点で捨てる(WASM のメモリは縮まないので、Workers の 128MB に収めるには描画中のピークを下げる)。ジオメトリやプロパティは要素ごとの小さな割り当てを避ける(MVT のプロパティはレイヤ共有のキー・値表へのタグ、グリフ範囲はテキストが使うコードポイントだけ保持)。1つのレンダラで描画を重ねて走らせるホストは、描画ごとに増加するチケットを `set_ticket` で渡し、`render_retaining(進行中で最古のチケット)` で描く(他の描画がまだ描いていないタイルを追い出さない。TS 側は描画の連番を使う)。
- **Sans-IO**: `core` は fetch / ファイル / async / スレッド / 時計に依存しない。`Renderer` が:
  - `needs() -> Vec<Resource>`: 現時点で必要な未取得リソース(tile / glyph range / sprite json / sprite png / TileJSON など)
  - `provide(url, Option<bytes>)`: 取得済みバイト列を渡す(`None` = 取得失敗/404)
  - `render() -> RGBA`: `needs()` が空のときのみ呼べる
- 必要リソースは**段階的に確定する**(タイル取得 → MVTデコードと式評価でテキスト確定 → glyph range 確定)。ホストは `needs()` が空になるまで取得と `provide()` を繰り返す。1往復で済む前提の設計にしない。
- `needs()` の段階: style → (TileJSON / GeoJSON / sprite json+png / image) → タイル → (欠落 raster の祖先 / グリフ範囲)。`provide` は現在のビューではなく `needs()` が返した URL の記録と照合する(`needs` と `provide` の間に `set_view` されても取りこぼさない)。`provide(None)` は上流の 404 と同じ意味(vector は空タイル、raster は祖先で代替、グリフ範囲は欠落)。
- 取得済みバイト列のキャッシュはホストの責務(TS 側は同一 URL の同時取得をまとめるだけ)。
- 単一スレッド。並列化しない。
- 採用済みクレート(すべて純 Rust・wasm32 確認済み): `serde_json`, `indexmap`, `unicode-normalization`, `earcut`(earcut.js 3.x の移植。MapLibre と同じ系統), `miniz_oxide`, `png`, `zune-jpeg`, `image-webp`, `i_overlay`(MVT 書き出しのポリゴン結合のみ)。描画は自前のソフトウェア GL(tiny-skia は不採用: GL の AA・合成規則と一致させるため)。
- テキストは SDF グリフを自前でしきい値処理して合成(halo / text-opacity もこの層)。MapLibre は HarfBuzz を使わず PBF グリフのメトリクスで自前レイアウトしているので、`shaping` / `placement` のアルゴリズムを再現する。RTL 等は後回し。

## 構成

- `crates/core/` — crate `ml2drender`。純粋ロジック(sans-IO)。
  - `expression/` — style-spec 式の移植(パース・型検査・評価)、`function.rs` は実行時の legacy function。
  - `style/` — レイヤ・プロパティのコンパイル、フィルタ。`spec.json` は v8.json の必要部分(`scripts/trim-spec.ts` で生成)。
  - `mvt.rs`(MVT デコード・エンコード、overzoom 分割)、`geojson.rs`(geojson-vt 移植)、`supercluster.rs`(GeoJSON クラスタ)、`tile.rs`(Transform・タイル被覆・URL)、`projection.rs`(投影式と PROJJSON の解釈。内部では PROJJSON を PROJ 文字列相当のパラメータに落として読む)、`reproject.rs`(raster / DEM の warp。頂点から描くレイヤは `Transform::reprojected` 経由で出力座標へ)、`image.rs`(画像デコード・GL 風テクスチャ)、`sprite.rs`(sprite 読み込み)。
  - `render/` — `raster.rs` がソフトウェア GL(三角形・GL ライン・合成・マスク)、`mod.rs` / `line.rs` / `raster_layer.rs` / `pattern.rs` が各レイヤのバケット生成とシェーダ再現。
  - `symbol/` — `glyphs.rs`(PBF グリフ)、`shaping.rs`、`layout.rs`(バケット・quad・衝突ボックス)、`placement.rs`(衝突判定)、`projection.rs`(line ラベル)、`draw.rs`(SDF/アイコンのシェーダ)。
  - 方針: MapLibre の **バケット生成とシェーダの計算を移植**し(ジオメトリ・式・量子化)、GL と同じ規則でラスタライズする(tiny-skia は使わない)。GPU パイプラインの構造は真似せず、出力が変わらない範囲で独自に組み替えてよい(深度バッファの代わりに画素ごとの最上位不透明レイヤを記録、varying の平面補間、バケットの描画間再利用など)。pitch 依存の計算は pitch 0 に簡約する。上流の量子化・桁あふれ等の癖も再現する(例: extrude の Int8 桁あふれ、GL の top-left 規則を y-down バッファでは「左辺+下辺」に読み替え、`linesofar` の量子化、overzoom 分割 `zoomLevelsToOverscale`=4 と vt-pbf 再エンコードの切り捨て、衝突ボックスの Int16 切り捨て、layout 同一レイヤのバケット共有)。
- `crates/cli/` — crate `ml2drender-cli`、バイナリ `ml2drender`。テストランナー兼ネイティブ検証用。
- `crates/wasm/` — crate `ml2drender-wasm`。wasm-bindgen なしの素の C-ABI export(`renderer_new/set_view/set_projected_view/set_ticket/set_keep_buckets/set_export_source/needs/provide/render/export_mvt/free`, `projection_new/forward/inverse/free`(結果は `xy_ptr`), `encode_png/encode_webp`, `alloc/dealloc`, RGBA / PNG・エラー等の結果は `out_ptr/out_len` の共有バッファ)。
- `packages/ml2drender/` — TypeScript の公開 API と型テスト。`src/index.ts` が WASM のロード・メモリ操作・リソース取得を隠す。描画は Mercator 前提の `renderView`(center / zoom / bearing)・`renderTile`(XYZ タイル)・`render`(経緯度 bbox と長辺の画素数。縦横比とズームは bbox から決める)と、`renderer.projected(crs)`(crs = GL JS の `addProjection` と同じ `name` / `tileMatrixSet` + PROJJSON)の `render`(bbox)・`renderTile`(格子の z/x/y)に分け、どれも RGBA の `RenderedImage` を返し、`image.toPng()` / `toWebp()` で WASM 内エンコードする(形式が増えても `toXxx()` を足すだけ)。`projected(crs).getTile`(格子の z/x/y + source。bbox は TS 側で計算、512px 固定)は描画ではなく、スタイルの 1 ソースを GL JS がそのまま読める形で再投影して返す(core は `set_export_source` → `needs`/`provide` → `export_tile`)。vector / GeoJSON は MVT(線・点は元タイルごとに切る。ポリゴンは元タイル内に収まるものはそのまま、緩衝帯だけのものは捨て、タイル縁で切られた断片はレイヤ・属性ごとに `i_overlay` で結合してつながった部分ごとの地物にしてから投影し、元タイル境界の継ぎ目を出さない(OpenMapTiles の海などは ID がタイルごとに違うので ID では束ねない)。定義域外(geos の裏側)は縁を二分探索し、ポリゴンは d3-geo の clip と同じく縁に沿って時計回りにつなぎ直す。リングは外周→その穴の順に並べる(読み手は最初のリングの向きを外周とみなす)。並べるのは出力範囲で切る前(切った後は外周と穴が切り口の辺を共有し、内外判定が曖昧になる)。スタイルで絞らず全レイヤ・全属性。一度書き出した vector ソースのタイルは全レイヤを保持する)。raster / image は既定 paint の raster レイヤで描いた Mercator の絵を warp した PNG、raster-dem は出力画素ごとの Mercator 位置で backfill 済み DEM の標高を双線形補間し terrarium で符号化した PNG。`projected(crs).toStyle({ tiles })` は init に渡した元のスタイル(fill-extrusion を平らにする前)のソースを GL JS 用に差し替えて返す。`loadProjection(crs)` は描画と同じ Rust の投影式(forward / inverse)を返す(タイル範囲の計算や GL JS の `addProjection` 用)。`init(..., { wasm })` でコンパイル済みモジュールを渡せる。`ttlMs: 0` のレンダラは描画ごとに WASM 側のレンダラを解放し、`loadProjection` とともにモジュールごとに1つの WASM インスタンスを共有する(解放したメモリを次の描画が再利用する。trap したインスタンスは捨てて次から作り直す)。リソースを保持するレンダラは専用インスタンス(GC で丸ごと回収)。`exclusive: true` は同じインスタンス上の描画を1本ずつ実行する(重なった描画が取得待ちのあいだデコード済みリソースを抱えて積み上がるのを防ぐ。Workers 向け)。
- `examples/browser/` — pnpm パッケージ `demo-browser`(Vite + TypeScript、maplibre-gl は npm 依存)。ml2drender の出力だけを GL JS で表示する。地図ペインの切り替えで、ml2drender が描いたラスタタイルと、GL JS 自身の描画(メインスレッドの `ttlMs: 0` のレンダラの `toStyle` で、全ソースを `ml2drender-source://` プロトコル経由の Worker の `getTile` に差し替える。投影なしでは EPSG:3857 の PROJJSON と GL JS の Mercator 格子を使う。ホバーで地物属性を表示)を選ぶ。左の Style JSON・PROJJSON・GeoJSON の入力が正(UI は英語)。各欄は見出しで開閉し、閉じているときはプルダウン(`src/styles.ts`、`src/projjson-presets.json`、GeoJSON は `main.ts` 内)だけ、開くと直接編集できる。入力は 500ms のデバウンスで自動反映し、Style / PROJJSON が変わったときだけタイルを描き直す(反映は直列化)。PROJJSON が空なら Web Mercator を再投影なしで描く。PROJJSON は `loadProjection` の投影式と、bbox(適用範囲)を投影した正方形のタイル格子で GL JS の `addProjection` に登録し(`src/projections.ts`)、各タイルを `projected(crs).renderTile`(空なら `renderTile`)で描く。投影の整合確認に GeoJSON 欄(既定は Natural Earth の都市)だけは GL JS 自身が描く(点は円、線・面も描く。ホバーで属性表示。変更は `setData` のみ)。GL JS は GeoJSON を Worker でタイル化するため、`src/gl-projection-handler.ts` を `importScriptInWorkers` で読み込み、`setProjectionHandler` に `loadProjectionFactory` の同期版を渡す(URL 末尾の `#.mjs` は GL JS にモジュールとして直接 import させるため、`vite.config.ts` の `worker.format: 'es'` は top-level await のため)。位置は URL ハッシュに残る。`needs`→fetch→`provide` ループのホスト実装例を兼ねる。JS API は `ml2drender` を `workspace:*` で参照。WASM は `pnpm build:wasm` で `packages/ml2drender/wasm/` にコピーし、API 内で `?url` import。社内標準のフロントエンド既定は SvelteKit だが、1画面の検証用ツールなので素の Vite にしている。
- `crates/cli/tests/exclusions.json` — 対象外 render テストの除外ルール(理由付き)。pitch/roll/非mercator/operations はランナー内にハードコード。
- `fixtures/` — 上流テスト(gitignore)。`scripts/fetch-fixtures.sh` が固定コミットから取得し LICENSE を同梱する。

## コマンド

全体を pnpm で管理する(`package.json` の scripts が cargo を包む)。Node は `.node-version`(26, Active LTS)、pnpm は `packageManager` で固定。`pnpm-workspace.yaml` で `minimumReleaseAge: 10080`(公開7日未満の版は入れない)。JS は TypeScript のみ(`.js` を新規に作らない)。

```sh
pnpm install
pnpm fixtures                      # 初回のみ(~144MB)
pnpm test                          # cargo test
pnpm test:render [filter]          # → target/test-report/render/{summary.md,results.json,<id>/diff.png}
pnpm test:expression [filter]      # → target/test-report/expression/{summary.md,failures.json}
pnpm bench base=<checkout> head=. # WASM の性能比較(fixture 使用、各 checkout で pnpm build:wasm 済み)。PR では .github/workflows/bench.yml が base と比較しコメント
pnpm demo                          # wasm ビルド + Vite dev server(examples/browser)
pnpm typecheck                     # tsc --noEmit(全パッケージ)
./target/release/ml2drender compare a.png b.png [threshold]
```

比較は maplibre-gl-js と同じ: pixelmatch 8.0.0 の移植(`crates/cli/src/pixelmatch.rs`、JS版と300組で一致確認済み)、`threshold` 既定 0.1285、`allowed`(不一致画素率)既定 0.00025、テストごとの `metadata.test` 指定があればそれに従う。`expected*.png` が複数あれば最小差分を採用。expression は上流と同じく数値を有効数字6桁で比較。

## ルール

- **テスト特化のハードコード、特定ケースへの分岐は禁止。** 仕様に基づく実装のみ。通らないケースは通らないまま残す。
- 差が大きい失敗(アンチエイリアスやジョイン形状以外)は個別に原因を調査する。アンチエイリアスやジョイン形状は MapLibre(GPU)と差が出るので許容差で吸収する。
- **高速化・省メモリ化は render テストの判定を変えない。** `pnpm test:render` の全件の判定が変更前と一致することを確認する(`results.json` の `hash` は比較した RGBA の SipHash なので、変更前の結果と突き合わせれば出力のビット一致も確かめられる)。メモリは WASM の `memory.buffer.byteLength`(ピーク)で測り、密なタイル(都市部の z14 など)でも確かめる。出力が変わる近似は、差が 8bit の量子化程度に収まるものに限り、差分率の変化と `pnpm bench` の出力差を PR に書く。ただし DEM(hillshade / color-relief)は画素単位の差も許容せず、RGBA をビット一致させる(libm を近似に置き換えない)。計測は `pnpm bench`(中央値)で前後を比較し、効果が誤差の範囲の試作は採用しない。
- **メルカトル固有の仮定を増やさない。** 将来は他の投影も対応する予定なので、最適化の適用可否は地理座標・緯度・bearing 0 ではなく投影後の画面形状から判断する。
- タイル単位のキャッシュは、中身が依存するものをすべてキーに含める(例: symbol バケットは layout グループ・wrap・ピクセル比、線/塗りは overscaled zoom)。`set_view` で使い回したレンダラと新しいレンダラの出力一致は `reused_renderer_matches_fresh_renders` で検査する。

## WASM

- `pnpm build:wasm` で専用 `wasm` プロファイル(opt-level 3 / fat LTO / codegen-units 1 / panic abort / strip)と simd128 を使用。成果物は `target/wasm32-unknown-unknown/wasm/` から公開パッケージへコピーする。
- 明示的 SIMD(`core::arch::wasm32`)はスカラー版と演算の種類・順序を揃え、`wasm32` かつ `simd128` のときだけ使う。ネイティブ(テストランナー)はスカラー版。

## 既知の非対応(仕様上・設計上の理由で通らないもの)

- 双方向テキスト(bidi)・アラビア文字の字形整形。
- 複合文字スクリプトや取得できなかったグリフ範囲のブラウザフォント描画(TinySDF)。フォントを持たないため再現不可。
- 配置の履歴依存(`prevAnchor`)、CrossTileSymbolIndex、retained tiles。単発の静止画描画では再現しない。
- ロケール別照合(照合はルート照合のみ)、`number-format` は en-US のみ、`is-supported-script` は常に true、feature-state は常に空。
- composite 式は表示ズームで直接評価(上流はタイルズーム前後の値をシェーダで補間。symbol のサイズのみ上流どおり)。
