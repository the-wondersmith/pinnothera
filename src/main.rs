// Pinnothera - a dead simple Kubernetes-native SNS/SQS configurator

use std::collections::BTreeMap;
use std::sync::Arc;

use aws_sdk_sns::Client as SNSClient;
use aws_sdk_sqs::Client as SQSClient;
use aws_types::SdkConfig as AWSConfig;
use easy_error::{bail, Terminator};
use k8s_openapi::api::core::v1::ConfigMap;
use kube::{api::Api, Client as K8sClient};
use once_cell::sync::OnceCell;
use serde::Deserialize;

// <editor-fold desc="// Global Statics ...">

static SNS_CLIENT: OnceCell<Arc<SNSClient>> = OnceCell::new();
static SQS_CLIENT: OnceCell<Arc<SQSClient>> = OnceCell::new();
static CLUSTER_ENV: OnceCell<Arc<EnvName>> = OnceCell::new();

// </editor-fold desc="// Global Statics ...">

// <editor-fold desc="// Structs & Enums ...">

#[derive(Clone, Debug, Default, Deserialize)]
struct SQSQueueConfig {
    topics: Vec<String>,
}

#[derive(Eq, Copy, Clone, Debug, PartialEq)]
enum EnvName {
    QA,
    Dev,
    Prod,
    Local,
    Unknown,
}

impl EnvName {
    fn from<T: AsRef<str>>(value: Option<T>) -> EnvName {
        if value.is_none() {
            return EnvName::Unknown;
        }

        match value.unwrap().as_ref().to_uppercase().as_str() {
            "L" | "LOCAL" => EnvName::Local,
            "Q" | "QA" | "QE" => EnvName::QA,
            "D" | "DEV" | "DEVELOPMENT" => EnvName::Dev,
            "P" | "PROD" | "PRODUCTION" => EnvName::Prod,
            _ => EnvName::Unknown,
        }
    }

    fn is_local(&self) -> bool {
        *self == EnvName::Local
    }

    fn is_unknown(&self) -> bool {
        *self == EnvName::Unknown
    }

    fn for_queue(&self) -> &str {
        match self {
            EnvName::QA => "qa",
            EnvName::Local => "local",
            EnvName::Dev => "development",
            EnvName::Prod => "production",
            EnvName::Unknown => "unknown",
        }
    }

    fn for_topic(&self) -> &str {
        match self {
            EnvName::QA => "qa",
            EnvName::Dev => "dev",
            EnvName::Prod => "prod",
            EnvName::Local => "local",
            EnvName::Unknown => "unknown",
        }
    }
}

impl Default for EnvName {
    fn default() -> Self {
        Self::Unknown
    }
}

impl<T: AsRef<str>> From<T> for EnvName {
    fn from(value: T) -> Self {
        EnvName::from(Some(value))
    }
}

// </editor-fold desc="// Structs & Enums ...">

// <editor-fold desc="// Type Aliases ...">

type SNSTopicARN = String;
type SQSQueueARN = String;
type SQSQueueURL = String;
type SQSQueueName = String;
type SNSSubscriptionARN = String;
type PinnConfig = BTreeMap<SQSQueueName, SQSQueueConfig>;

// </editor-fold desc="// Type Aliases ...">

// <editor-fold desc="// SNS Topic Utilities ...">

async fn create_topic<T: AsRef<str>>(topic: T) -> Result<SNSTopicARN, Terminator> {
    println!("Ensuring existence of topic: \"{}\"", topic.as_ref());

    let topic: String = if CLUSTER_ENV.get().unwrap().is_unknown() {
        topic.as_ref().to_string()
    } else {
        let suffix = CLUSTER_ENV.get().unwrap().for_topic();
        println!(
            "Suffixing topic \"{}\" as \"{}-{}\" per in-cluster configuration...",
            topic.as_ref(),
            topic.as_ref(),
            suffix
        );
        format!("{}-{}", topic.as_ref(), suffix, )
    };

    let resp = SNS_CLIENT
        .get()
        .unwrap()
        .create_topic()
        .name(&topic)
        .send()
        .await?;

    match resp.topic_arn() {
        None => {
            println!("Creation of topic \"{}\" did not return an error, but also did not return an ARN as expected",
                     &topic);
            bail!("")
        }
        Some(value) => {
            println!("Topic \"{}\" exists with ARN: \"{}\"", &topic, value, );
            Ok(value.to_string())
        }
    }
}

// </editor-fold desc="// SNS Topic Utilities ...">

// <editor-fold desc="// SQS Queue Utilities ...">

#[allow(unreachable_code)]
async fn create_queue<T: AsRef<str>>(queue: T) -> Result<(SQSQueueURL, SQSQueueARN), Terminator> {
    println!("Ensuring existence of queue: \"{}\"", queue.as_ref());

    let queue: String = if CLUSTER_ENV.get().unwrap().is_unknown() {
        queue.as_ref().to_string()
    } else {
        let suffix = CLUSTER_ENV.get().unwrap().for_queue();
        println!(
            "Suffixing queue \"{}\" as \"{}-{}\" per in-cluster configuration...",
            queue.as_ref(),
            queue.as_ref(),
            suffix
        );
        format!("{}-{}", queue.as_ref(), suffix, )
    };

    let resp = SQS_CLIENT
        .get()
        .unwrap()
        .create_queue()
        .queue_name(&queue)
        .send()
        .await?;

    let queue_url = match resp.queue_url() {
        Some(value) => value.to_string(),
        None => {
            println!(
                "Creation of queue \"{}\" did not return an error, but also did not return a URL as expected",
                queue
            );
            return bail!("");
        }
    };

    let attributes = SQS_CLIENT
        .get()
        .unwrap()
        .get_queue_attributes()
        .queue_url(queue_url.as_str())
        .attribute_names(aws_sdk_sqs::model::QueueAttributeName::QueueArn)
        .send()
        .await?
        .attributes
        .unwrap_or_default();

    let queue_arn = match attributes.get(&aws_sdk_sqs::model::QueueAttributeName::QueueArn) {
        None => {
            println!(
                "Creation of queue \"{}\" did return a url ({}), but also did not return an associated ARN as expected",
                &queue,
                &queue_url,
            );
            return bail!("");
        }
        Some(value) => {
            println!(
                "Queue \"{}\" exists with URL & ARN: [url: \"{}\", arn: \"{}\"]",
                &queue, &queue_url, value
            );
            value.to_string()
        }
    };

    Ok((queue_url, queue_arn))
}

// </editor-fold desc="// SQS Queue Utilities ...">

// <editor-fold desc="// SNS->SQS Subscription Utilities ...">

#[allow(unreachable_code)]
async fn create_subscription<T: AsRef<str>>(
    queue_arn: T,
    topic: T,
) -> Result<SNSSubscriptionARN, Terminator> {
    let topic_arn = create_topic(topic.as_ref()).await?;

    println!(
        "Ensuring queue \"{}\" is subscribed to topic [name: \"{}\", arn: \"{}\"] ...",
        topic.as_ref(),
        &topic_arn,
        queue_arn.as_ref(),
    );

    let subscription = SNS_CLIENT
        .get()
        .unwrap()
        .subscribe()
        .topic_arn(&topic_arn)
        .protocol("sqs")
        .endpoint(queue_arn.as_ref())
        .send()
        .await?;

    match subscription.subscription_arn {
        None => {
            println!(
                "Subscription of topic \"{}\" to queue ARN \"{}\" did not return an error,\
             but also did not return a subscription ARN either",
                topic.as_ref(),
                queue_arn.as_ref()
            );
            bail!("")
        }
        Some(arn) => {
            println!(
                "Queue \"{}\" is subscribed to topic w/ ARN: \"{}\"",
                queue_arn.as_ref(),
                &arn
            );
            Ok(arn)
        }
    }
}

async fn apply_queue_configuration<T: AsRef<str>>(
    queue: T,
    config: SQSQueueConfig,
) -> Result<(), Terminator> {
    // Create a convenient place to accumulate
    // the task handles we're about to create
    let mut tasks: Vec<_> = Vec::new();

    if queue.as_ref() == "unsubscribed" {
        // If the supplied queue is actually the sentinel value
        // "unsubscribed", just create the configured topics but
        // don't attempt to subscribe them to anything
        config.topics.into_iter().for_each(|topic| {
            tasks.push(tokio::spawn(async { create_topic(topic).await.expect("") }));
        });
    } else {
        // Get the specified queue's URL and ARN
        let (_queue_url, queue_arn) = create_queue(queue).await?;

        // Create the queue's required subscriptions
        config.topics.into_iter().for_each(|topic| {
            let task_arn = queue_arn.clone();
            tasks.push(tokio::spawn(async move {
                create_subscription(task_arn, topic).await.expect("")
            }));
        })
    }

    // Await all of the created handles in parallel
    futures_util::future::join_all(tasks).await;

    Ok(())
}

// </editor-fold desc="// SNS->SQS Subscription Utilities ...">

// <editor-fold desc="// Kubernetes API Utilities ...">

async fn pinn_config_from_json(data: &String) -> Result<PinnConfig, Terminator> {
    match serde_json::from_str::<PinnConfig>(data.as_str()) {
        Ok(obj) => Ok(obj),
        Err(error) => {
            println!("Couldn't deserialize JSON data: {}", data);
            Err(error.into())
        }
    }
}

async fn pinn_config_from_yaml(data: &String) -> Result<PinnConfig, Terminator> {
    match serde_yaml::from_str::<PinnConfig>(data.as_str()) {
        Ok(obj) => Ok(obj),
        Err(error) => {
            println!("Couldn't deserialize YAML data: {}", data);
            Err(error.into())
        }
    }
}

async fn get_pinn_config() -> Result<(EnvName, PinnConfig), Terminator> {
    // Infer and create a Kubernetes `Client` from the runtime environment
    let client: K8sClient = K8sClient::try_default().await?;

    // Read `ConfigMap`s in the configured namespace
    // into the typed interface from k8s-openapi
    let config_maps: Api<ConfigMap> = Api::default_namespaced(client);

    // Use the typed interface to pull the namespace's
    // pinnothera configuration (if it exists)
    let pinn_confmap: ConfigMap = match config_maps.get_opt("sns-sqs-config").await? {
        Some(obj) => obj,
        None => {
            println!("No pinnothera-recognized ConfigMap in current cluster namespace!");
            return Ok((EnvName::Unknown, BTreeMap::new()));
        }
    };

    // Pull out the ConfigMap's `annotations` element (if it exists)
    let annotations: BTreeMap<String, String> = match pinn_confmap.metadata.annotations {
        Some(obj) => obj,
        None => BTreeMap::new(),
    };

    let env_name: EnvName = EnvName::from(annotations.get("app-env"));

    // Pull out the ConfigMap's `data` element (if it exists)
    let confs_map: BTreeMap<String, String> = match pinn_confmap.data {
        Some(obj) => obj,
        None => {
            println!(
                "The `sns-sqs-config` ConfigMap retrieved from the current cluster namespace has no `data` element!"
            );
            return Ok((EnvName::Unknown, BTreeMap::new()));
        }
    };

    // Parse the data from the first recognized key and return it
    if let Some(data) = confs_map.get("json") {
        return match pinn_config_from_json(data).await {
            Ok(config) => Ok((env_name, config)),
            Err(error) => Err(error),
        };
    } else if let Some(data) = confs_map.get("yaml") {
        return match pinn_config_from_yaml(data).await {
            Ok(config) => Ok((env_name, config)),
            Err(error) => Err(error),
        };
    };

    println!("The `data` element in the `sns-sqs-config` ConfigMap retrieved from the current cluster namespace has no pinnothera-recognized keys!");

    Ok((EnvName::Unknown, BTreeMap::new()))
}

// </editor-fold desc="// Kubernetes API Utilities ...">

// <editor-fold desc="// AWS Configuration Utilities ...">

async fn aws_client_configs_for_env() -> (aws_sdk_sns::config::Config, aws_sdk_sqs::config::Config)
{
    // Infer and create an AWS `Config` from the current environment
    let config: AWSConfig = aws_config::load_from_env().await;

    let (mut sns_config, mut sqs_config) = (
        aws_sdk_sns::config::Builder::from(&config),
        aws_sdk_sqs::config::Builder::from(&config),
    );

    if CLUSTER_ENV.get().unwrap().is_local() {
        sns_config.set_endpoint_resolver(Some(Arc::new(
            aws_smithy_http::endpoint::Endpoint::immutable(http::Uri::from_static(
                "http://aws.localstack",
            )),
        )));
        sqs_config.set_endpoint_resolver(Some(Arc::new(
            aws_smithy_http::endpoint::Endpoint::immutable(http::Uri::from_static(
                "http://aws.localstack",
            )),
        )));
    }

    (sns_config.build(), sqs_config.build())
}

// </editor-fold desc="// AWS Configuration Utilities ...">

// <editor-fold desc="// Main ...">

#[tokio::main]
async fn main() -> Result<(), Terminator> {
    // Get the SNS/SQS topic & queue configuration from the
    // cluster (if it exists in the current namespace)
    let (env_name, pinn_config) = get_pinn_config().await?;

    println!("Applying queue configuration: {:#?}", &pinn_config);

    CLUSTER_ENV.set(Arc::new(env_name)).unwrap();

    // Get a usable AWS configuration objects for the local environment
    let (sns_config, sqs_config) = aws_client_configs_for_env().await;

    // Use the inferred AWS config to create SNS and SQS clients
    let sns_client: SNSClient = SNSClient::from_conf(sns_config);
    let sqs_client: SQSClient = SQSClient::from_conf(sqs_config);

    // Allow the created clients to be accessed globally safely via
    // the `OnceCell`s created as part of pinnothera's initialization
    SNS_CLIENT.set(Arc::new(sns_client)).unwrap();
    SQS_CLIENT.set(Arc::new(sqs_client)).unwrap();

    // Spawn async tasks to apply the parsed queue & topic configurations
    let tasks: Vec<_> = pinn_config
        .into_iter()
        .map(|(queue, queue_config)| {
            tokio::spawn(async move {
                apply_queue_configuration(queue, queue_config)
                    .await
                    .unwrap()
            })
        })
        .collect();

    // Wait for all of the spawned tasks to finish
    futures_util::future::join_all(tasks).await;

    Ok(())
}

// </editor-fold desc="// Main ...">
