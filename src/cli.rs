// Pinnothera's command line argument parsing components

use std::fmt::Formatter;
// Standard Library Imports
use std::path::PathBuf;
use std::sync::Arc;

// Third Party Imports
use aws_sdk_sns::config::Config as SNSClientConfig;
use aws_sdk_sqs::config::Config as SQSClientConfig;
use aws_types::credentials::{
    future::ProvideCredentials as ProvideAWSCredentials, Credentials as AWSCredentials,
    CredentialsError as AWSCredentialsError, ProvideCredentials as AWSCredentialProvider,
    SharedCredentialsProvider as SharedAWSCredentialsProvider,
};
use aws_types::{region::Region, SdkConfig as AWSConfig};
use clap::Parser;
use easy_error::{bail, Terminator};
use kube::Client as K8sClient;

// Project-Level Imports
use crate::{EnvName, PinnConfig, CLUSTER_ENV};

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

    /// Name of the name of the "environment" the target
    /// cluster is running in (i.e. 'dev' or 'production')
    #[clap(short = 'e', long = "env-name", value_parser)]
    pub(crate) env_name: Option<String>,

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
    pub(crate) aws_access_key_id: Option<String>,

    /// The Secret Access Key that pinnothera should use
    /// to communicate with AWS SNS/SQS services
    #[clap(long = "aws-secret-access-key", value_parser)]
    pub(crate) aws_secret_access_key: Option<String>,

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

struct CLICredentialProvider {
    access_key_id: String,
    secret_access_key: String,
}

impl std::fmt::Debug for CLICredentialProvider {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "CLICredentialProvider(access_key_id: {}, secret_access_key: {},",
            self.access_key_id.as_str(),
            self.secret_access_key.as_str()
        )
    }
}

impl CLICredentialProvider {
    async fn aws_credentials(&self) -> aws_types::credentials::Result {
        Ok(AWSCredentials::new(
            self.access_key_id.as_str(),
            self.secret_access_key.as_str(),
            None,
            None,
            "Pinnothera CLI arguments",
        ))
    }
}

impl AWSCredentialProvider for CLICredentialProvider {
    fn provide_credentials<'a>(&'a self) -> ProvideAWSCredentials<'a>
    where
        Self: 'a,
    {
        ProvideAWSCredentials::new(self.aws_credentials())
    }
}

impl TryFrom<&CLIArgs> for CLICredentialProvider {
    type Error = AWSCredentialsError;

    fn try_from(args: &CLIArgs) -> Result<Self, Self::Error> {
        if !args.aws_access_key_id.is_some() {
            Err(AWSCredentialsError::provider_error(
                "Missing or empty access key id!",
            ))
        } else if !args.aws_secret_access_key.is_some() {
            Err(AWSCredentialsError::provider_error(
                "Missing or empty secret access key!",
            ))
        } else {
            Ok(CLICredentialProvider {
                access_key_id: args.aws_access_key_id.as_ref().unwrap().clone(),
                secret_access_key: args.aws_secret_access_key.as_ref().unwrap().clone(),
            })
        }
    }
}

impl CLIArgs {
    // <editor-fold desc="// AWS Configuration Utilities ...">
    pub async fn aws_client_configs(
        &'static self,
    ) -> Result<(SNSClientConfig, SQSClientConfig), Terminator> {
        if self.aws_role_arn.is_some() {
            bail!("Support for explicit AWS Role ARNs not yet implemented!")
        }
        // Infer and create an AWS `Config` from the current environment
        let config: AWSConfig = aws_config::load_from_env().await;

        let (sns_config, sqs_config) = (
            aws_sdk_sns::config::Builder::from(&config),
            aws_sdk_sqs::config::Builder::from(&config),
        );

        let (mut sns_config, mut sqs_config) = match &self.aws_region {
            Some(region) => (
                sns_config.region(Region::new(region)),
                sqs_config.region(Region::new(region)),
            ),
            None => (sns_config, sqs_config),
        };

        let endpoint = if let Some(url) = &self.aws_endpoint {
            Some(url.as_str())
        } else if CLUSTER_ENV
            .get()
            .unwrap_or(&Arc::new(EnvName::Unknown))
            .is_local()
        {
            Some("http://aws.localstack")
        } else {
            None
        };

        if let Some(url) = endpoint {
            sns_config.set_endpoint_resolver(Some(Arc::new(
                aws_smithy_http::endpoint::Endpoint::immutable(http::Uri::from_static(url)),
            )));
            sqs_config.set_endpoint_resolver(Some(Arc::new(
                aws_smithy_http::endpoint::Endpoint::immutable(http::Uri::from_static(url)),
            )));
        }

        if self.aws_access_key_id.is_some() & self.aws_secret_access_key.is_some() {
            sns_config.set_credentials_provider(Some(SharedAWSCredentialsProvider::new(
                CLICredentialProvider::try_from(self)?,
            )));
            sqs_config.set_credentials_provider(Some(SharedAWSCredentialsProvider::new(
                CLICredentialProvider::try_from(self)?,
            )));
        }

        Ok((sns_config.build(), sqs_config.build()))
    }

    // </editor-fold desc="// AWS Configuration Utilities ...">

    // <editor-fold desc="// Kubernetes Configuration Utilities ...">

    async fn kube_config(&self) -> Result<kube::Config, Terminator> {
        let options = kube::config::KubeConfigOptions {
            context: self.kube_context.clone(),
            cluster: None,
            user: None,
        };

        let config =
            kube::Config::from_custom_kubeconfig(kube::config::Kubeconfig::read()?, &options)
                .await?;

        Ok(config)
    }

    // </editor-fold desc="// Kubernetes Configuration Utilities ...">

    // <editor-fold desc="// Pinnothera Configuration Utilities ...">

    pub async fn pinn_config(&mut self) -> Result<(EnvName, PinnConfig), Terminator> {
        if let Some(json_path) = &self.json_file {
            self.json_data = Some(tokio::fs::read_to_string(json_path).await?);
        } else if let Some(yaml_path) = &self.yaml_file {
            self.yaml_data = Some(tokio::fs::read_to_string(yaml_path).await?);
        }

        if let Some(json_data) = &self.json_data {
            return Ok((
                EnvName::from(self.env_name.clone()),
                PinnConfig::from_json(json_data)?,
            ));
        } else if let Some(yaml_data) = &self.yaml_data {
            return Ok((
                EnvName::from(self.env_name.clone()),
                PinnConfig::from_yaml(yaml_data)?,
            ));
        }

        let client = match self.kube_context {
            None => K8sClient::try_default().await?,
            Some(_) => K8sClient::try_from(self.kube_config().await?)?,
        };

        PinnConfig::from_cluster(
            client,
            &self.env_name,
            &self.namespace,
            &self.configmap_name,
        )
        .await
    }

    // </editor-fold desc="// Pinnothera Configuration Utilities ...">
}
