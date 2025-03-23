# Chiitiler Lambda Function URL CDK Project

> [!WARNING]
> This cdk is very simple example to demonstrate how to deploy chiitiler on AWS Lambda with Container Image. You shouldn't expose raw chiitiler to public, proper AWS IAM settings or using as one of microservices is highly recommended.

```sh
cd cdk
npx cdk deploy
# check your cpu architecture, default to arm64
```

Then, you will acquire a function URL from the output.

```sh
Outputs:
ChiitilerStack.ChiitilerFunctionUrl = https://---suppressed---.lambda-url.ap-northeast-1.on.aws/
```

Then, you can use chiitiler endpoints:

```sh
wget https://---suppressed---.lambda-url.ap-northeast-1.on.aws/tiles/0/0/0.png?url=https://tile.openstreetmap.jp/styles/osm-bright/style.json
wget https://---suppressed---.lambda-url.ap-northeast-1.on.aws/clip.png?bbox=100,30,150,60&url=https://tile.openstreetmap.jp/styles/osm-bright/style.json
```
