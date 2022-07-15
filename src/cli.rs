// Pinnothera's command line argument parsing components

use std::path::PathBuf;

use aws_types::SdkConfig as AWSConfig;
use clap::Parser;

// const CLI_ABOUT: &str = "";

/// A dead simple Kubernetes-native SNS/SQS configurator
#[derive(Parser, Debug)]
#[clap(version, about, long_about = None)]
pub(crate) struct CLIArgs {
    // <editor-fold desc="// Kubernetes-related Settings ...">
    /// Name of the Kubernetes `Namespace` containing
    /// the SNS/SQS configuration pinnothera should apply
    #[clap(short = 'n', long = "namespace", value_parser)]
    pub(crate) namespace: Option<String>,

    /// Name of the Kubernetes `ConfigMap` containing
    /// the SNS/SQS configuration pinnothera should apply
    #[clap(short = 'm', long = "configmap", default_value_t = String::from("sns-sqs-config"), value_parser)]
    pub(crate) configmap_name: String,

    /// Name of the name of the `kubectl` "context"
    /// pinnothera should use when communicating with
    /// the target cluster
    #[clap(short = 'c', long = "kube-context", value_parser)]
    pub(crate) kube_context: Option<String>,

    // </editor-fold desc="// Kubernetes-related Settings ...">

    // <editor-fold desc="// AWS-related Settings ...">
    /// The AWS region name pinnothera should use
    /// to communicate with SNS/SQS services
    #[clap(long = "aws-region", value_parser)]
    pub(crate) aws_region: Option<String>,

    /// The "endpoint" that pinnothera should use
    /// to communicate with AWS SNS/SQS services
    #[clap(long = "aws-endpoint", value_parser)]
    pub(crate) aws_endpoint: Option<String>,

    /// The Role ARN that pinnothera should use
    /// to communicate with AWS SNS/SQS services
    #[clap(long = "aws-role-arn", value_parser)]
    pub(crate) aws_role_arn: Option<String>,

    /// The Secret Key ID that pinnothera should use
    /// to communicate with AWS SNS/SQS services
    #[clap(long = "aws-access-key-id", value_parser)]
    pub(crate) aws_secret_key_id: Option<String>,

    /// The Secret Access Key that pinnothera should use
    /// to communicate with AWS SNS/SQS services
    #[clap(long = "aws-secret-access-key", value_parser)]
    pub(crate) aws_secret_access_id: Option<String>,

    // </editor-fold desc="// AWS-related Settings ...">

    // <editor-fold desc="// Raw Config Data Settings ...">
    /// JSON-serialized string containing the SNS/SQS
    /// configuration pinnothera should apply
    #[clap(long = "json-data", value_parser)]
    pub(crate) json_data: Option<String>,

    /// YAML-serialized string containing the SNS/SQS
    /// configuration pinnothera should apply
    #[clap(long = "yaml-data", value_parser)]
    pub(crate) yaml_data: Option<String>,

    /// Absolute or relative on-disk path to a file
    /// containing JSON-serialized SNS/SQS configuration
    /// data that pinnothera should apply
    #[clap(long = "json-file", value_parser)]
    pub(crate) json_file: Option<PathBuf>,

    /// Absolute or relative on-disk path to a file
    /// containing YAML-serialized SNS/SQS configuration
    /// data that pinnothera should apply
    #[clap(long = "yaml-file", value_parser)]
    pub(crate) yaml_file: Option<PathBuf>,
    // </editor-fold desc="// Raw Config Data Settings ...">
}

impl CLIArgs {
    pub async fn aws_config(&self) -> AWSConfig {
        // let config: AWSConfig = aws_types::SdkConfig::builder()
        //     .region(aws_types::region::Region::new("us-east-2"))
        //     .credentials_provider(LocalstackCredentialProvider::new_shared_provider())
        //     .endpoint_resolver(aws_smithy_http::endpoint::Endpoint::immutable(
        //         http::Uri::from_static("http://aws.localstack"),
        //     ))
        //     .build();
        todo!()
    }
}
