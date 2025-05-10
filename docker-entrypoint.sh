#!/bin/sh

# Xvfbを適切なディスプレイ番号と設定でバックグラウンド起動
# 例: ディスプレイ番号 :99, 解像度 1280x1024, 色深度 24bit
Xvfb :99 -screen 0 1280x1024x24 -ac -nolisten tcp -nolisten unix &
XVFB_PID=$! # XvfbのプロセスIDを保持（オプション）

# アプリケーションが使用するDISPLAY環境変数を設定
export DISPLAY=:99

echo "Xvfb started with DISPLAY=${DISPLAY} (PID: ${XVFB_PID})"

# DockerfileのCMDで指定されたコマンド（この場合はNode.jsアプリ）を実行
# exec "$@" を使うことで、Node.jsアプリがコンテナのメインプロセス(PID 1)となり、
# Dockerからのシグナルを正しく受け取れるようにする
exec "$@"

# コンテナ終了時にXvfbをクリーンアップする処理（オプション）
# trap "echo 'Stopping Xvfb (PID: ${XVFB_PID})'; kill ${XVFB_PID}; exit" SIGINT SIGTERM