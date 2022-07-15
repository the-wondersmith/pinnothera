// Pinnothera - a dead simple Kubernetes-native SNS/SQS configurator

#![allow(dead_code, unused_imports)]

// Standard Library
use std::sync::Arc;

// Third Party
use aws_sdk_sns::Client as SNSClient;
use aws_sdk_sqs::Client as SQSClient;
use aws_types::SdkConfig as AWSConfig;
use clap;
use easy_error::{bail, Terminator};
use once_cell::sync::OnceCell;

// Crate-local
use cli::CLIArgs;
use types::{
    EnvName, PinnConfig, SNSSubscriptionARN, SNSTopicARN, SQSQueueARN, SQSQueueConfig,
    SQSQueueName, SQSQueueURL,
};

mod cli;
mod types;

// <editor-fold desc="// Global Statics ...">

static SNS_CLIENT: OnceCell<Arc<SNSClient>> = OnceCell::new();
static SQS_CLIENT: OnceCell<Arc<SQSClient>> = OnceCell::new();
static CLUSTER_ENV: OnceCell<Arc<EnvName>> = OnceCell::new();

// </editor-fold desc="// Global Statics ...">

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
        format!("{}-{}", topic.as_ref(), suffix,)
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
            println!("Topic \"{}\" exists with ARN: \"{}\"", &topic, value,);
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
        format!("{}-{}", queue.as_ref(), suffix,)
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
    let args = <CLIArgs as clap::Parser>::parse();

    println!("{:#?}", args);

    // // Get the SNS/SQS topic & queue configuration from the
    // // cluster (if it exists in the current namespace)
    // let (env_name, pinn_config) = PinnConfig::from_cluster().await?;
    //
    // println!("Applying queue configuration: {:#?}", &pinn_config);
    //
    // CLUSTER_ENV.set(Arc::new(env_name)).unwrap();
    //
    // // Get a usable AWS configuration objects for the local environment
    // let (sns_config, sqs_config) = aws_client_configs_for_env().await;
    //
    // // Use the inferred AWS config to create SNS and SQS clients
    // let sns_client: SNSClient = SNSClient::from_conf(sns_config);
    // let sqs_client: SQSClient = SQSClient::from_conf(sqs_config);
    //
    // // Allow the created clients to be accessed globally safely via
    // // the `OnceCell`s created as part of pinnothera's initialization
    // SNS_CLIENT.set(Arc::new(sns_client)).unwrap();
    // SQS_CLIENT.set(Arc::new(sqs_client)).unwrap();
    //
    // // Spawn async tasks to apply the parsed queue & topic configurations
    // let tasks: Vec<_> = pinn_config
    //     .into_iter()
    //     .map(|(queue, queue_config)| {
    //         tokio::spawn(async move {
    //             apply_queue_configuration(queue, queue_config)
    //                 .await
    //                 .unwrap()
    //         })
    //     })
    //     .collect();
    //
    // // Wait for all of the spawned tasks to finish
    // futures_util::future::join_all(tasks).await;

    Ok(())
}

// </editor-fold desc="// Main ...">
