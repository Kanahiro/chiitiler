import * as cdk from 'aws-cdk-lib';
import * as lambda from 'aws-cdk-lib/aws-lambda';
import * as s3 from 'aws-cdk-lib/aws-s3';
import { Construct } from 'constructs';

export class ChiitilerStack extends cdk.Stack {
	constructor(scope: Construct, id: string, props?: cdk.StackProps) {
		super(scope, id, props);

		// Create S3 bucket for caching
		const cacheBucket = new s3.Bucket(this, 'ChiitilerCacheBucket', {
			bucketName: 'chiitiler-cache',
			removalPolicy: cdk.RemovalPolicy.DESTROY,
			autoDeleteObjects: true,
			lifecycleRules: [
				// delete objects older than 1 days
				{
					expiration: cdk.Duration.days(1),
				},
			],
		});

		// Create the Lambda function using the Docker image
		const chiitilerFunction = new lambda.DockerImageFunction(
			this,
			'ChiitilerFunction',
			{
				architecture: lambda.Architecture.ARM_64,
				code: lambda.DockerImageCode.fromImageAsset('../'),
				description: 'Chiitiler - The Tiny MapLibre Server',
				timeout: cdk.Duration.seconds(30),
				memorySize: 1024,
				environment: {
					// environment variables for chiitiler
					CHIITILER_PORT: '3000',
					CHIITILER_STREAM_MODE: 'true', // Enable stream mode for Lambda
					CHIITILER_CACHE_METHOD: 's3', // Use S3 cache
					CHIITILER_S3CACHE_BUCKET: cacheBucket.bucketName,
					CHIITILER_S3_REGION: 'ap-northeast-1',
				},
			},
		);

		cacheBucket.grantReadWrite(chiitilerFunction);

		// Create a Function URL for the Lambda function
		const functionUrl = chiitilerFunction.addFunctionUrl({
			authType: lambda.FunctionUrlAuthType.NONE, // Public access
			invokeMode: lambda.InvokeMode.RESPONSE_STREAM,
		});

		// Output the Function URL
		new cdk.CfnOutput(this, 'ChiitilerFunctionUrl', {
			value: functionUrl.url,
			description: 'URL for the Chiitiler Lambda Function',
		});
	}
}
