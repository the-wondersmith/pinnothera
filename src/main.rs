// Pinnothera - a dead simple Kubernetes-native SNS/SQS configurator

// Standard Library Imports
use std::process::ExitCode;
use std::sync::Arc;

// Third Party Imports
use aws_sdk_sns::Client as SNSClient;
use aws_sdk_sqs::Client as SQSClient;
use clap;
use easy_error::{bail, Terminator};
use once_cell::sync::OnceCell;

// Project-Level Imports
pub(crate) use cli::CLIArgs;
pub(crate) use types::{
    EnvName, PinnConfig, SNSTopicARN, SQSQueueARN, SQSQueueConfig, SQSQueueURL,
};

pub(crate) mod cli;
pub(crate) mod types;

// <editor-fold desc="// Global Statics ...">

pub(crate) static CLUSTER_ENV: OnceCell<Arc<EnvName>> = OnceCell::new();
pub(crate) static SNS_CLIENT: OnceCell<Arc<SNSClient>> = OnceCell::new();
pub(crate) static SQS_CLIENT: OnceCell<Arc<SQSClient>> = OnceCell::new();
pub(crate) static PINN_CONFIG: OnceCell<Arc<PinnConfig>> = OnceCell::new();
pub(crate) static CLI_ARGS: OnceCell<Arc<CLIArgs>> = OnceCell::new();

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

    let resp = match SNS_CLIENT
        .get()
        .unwrap()
        .create_topic()
        .name(&topic)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            println!("Could not create topic due to error:\n----- Create '{}' Error -----\n{:#?}\n----- Create '{}' Error -----\n", &topic, &error, &topic, );
            return Err(error.into());
        }
    };

    match resp.topic_arn() {
        None => {
            println!("Creation of topic \"{}\" did not return an error, but did not return an ARN as expected",
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

    let resp = match SQS_CLIENT
        .get()
        .unwrap()
        .create_queue()
        .queue_name(&queue)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            println!("Could not create queue due to error:\n----- Create '{}' Error -----\n{:#?}\n----- Create '{}' Error -----\n", &queue, &error, &queue, );
            return Err(error.into());
        }
    };

    let queue_url = match resp.queue_url() {
        Some(value) => value.to_string(),
        None => {
            println!(
                "Creation of queue \"{}\" did not return an error, but did not return a URL as expected",
                queue
            );
            bail!("")
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
                "Creation of queue \"{}\" did return a url ({}), but did not return an associated ARN as expected",
                &queue,
                &queue_url,
            );
            bail!("")
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

async fn create_subscription<T: AsRef<str>>(queue_arn: T, topic: T) -> Result<u8, u8> {
    let (queue_arn, topic): (&str, &str) = (queue_arn.as_ref(), topic.as_ref());
    let topic_arn = match create_topic(topic).await {
        Ok(arn) => arn,
        Err(_) => {
            return Err(1);
        }
    };

    println!(
        "Ensuring queue \"{}\" is subscribed to topic [name: \"{}\", arn: \"{}\"] ...",
        topic, &topic_arn, queue_arn,
    );

    let subscription = match SNS_CLIENT
        .get()
        .unwrap()
        .subscribe()
        .topic_arn(&topic_arn)
        .protocol("sqs")
        .endpoint(queue_arn)
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            println!("Could not ensure subscription of queue to topic due to error:\n----- Subscribe '{}' to '{}' Error -----\n{:#?}\n----- Subscribe '{}' to '{}' Error -----\n", queue_arn, topic, &error, queue_arn, topic, );
            return Err(1);
        }
    };

    match subscription.subscription_arn {
        None => {
            println!(
                "Subscription of topic \"{}\" to queue ARN \"{}\" did not return an error,\
             but did not return a subscription ARN either",
                topic, queue_arn
            );
            Err(1)
        }
        Some(arn) => {
            println!(
                "Queue \"{}\" is subscribed to topic w/ ARN: \"{}\"",
                queue_arn, &arn
            );
            Ok(0)
        }
    }
}

async fn apply_queue_configuration<T: AsRef<str>>(
    queue: T,
    config: &'static SQSQueueConfig,
) -> Result<u8, u8> {
    // Create a convenient place to accumulate
    // the task handles we're about to create
    let mut tasks: Vec<_> = Vec::new();

    if queue.as_ref() == "unsubscribed" {
        // If the supplied queue is actually the sentinel value
        // "unsubscribed", just create the configured topics but
        // don't attempt to subscribe them to anything
        config.topics.iter().for_each(|topic| {
            tasks.push(tokio::spawn(async {
                match create_topic(topic.to_string()).await {
                    Ok(_) => 0,
                    Err(_) => 1,
                }
            }));
        });
    } else {
        // Get the specified queue's URL and ARN
        let (_queue_url, queue_arn) = match create_queue(queue).await {
            Ok((url, arn)) => (url, arn),
            Err(_) => {
                return Err(1);
            }
        };

        // Create the queue's required subscriptions
        config.topics.iter().for_each(|topic| {
            let task_arn = queue_arn.clone();
            tasks.push(tokio::spawn(async move {
                create_subscription(task_arn, topic.to_string())
                    .await
                    .unwrap()
            }));
        })
    }

    // Await all of the created handles in parallel
    let results: Vec<u8> = futures_util::future::join_all(tasks)
        .await
        .iter()
        .map(|result| match result {
            Ok(value) => *value,
            Err(_) => 1 as u8,
        })
        .collect();

    Ok(results.iter().sum::<u8>())
}

// </editor-fold desc="// SNS->SQS Subscription Utilities ...">

// <editor-fold desc="// Main ...">

#[tokio::main]
async fn main() -> ExitCode {
    // Parse and store any cli arguments that were supplied
    let mut args: CLIArgs = <CLIArgs as clap::Parser>::parse();

    // Get the SNS/SQS topic & queue configuration from the
    // cluster (if it exists in the current namespace)
    let (env_name, pinn_config) = match args.pinn_config().await {
        Ok((name, config)) => (name, config),
        Err(error) => {
            println!(
                "\n\n{:#?}\n\nCould not parse or acquire usable pinnothera configuration due to ^\n\n",
                error
            );
            return ExitCode::from(2);
        }
    };

    println!("Applying queue configuration: {:#?}", &pinn_config);

    PINN_CONFIG.set(Arc::new(pinn_config)).unwrap();
    CLUSTER_ENV.set(Arc::new(env_name)).unwrap();
    CLI_ARGS.set(Arc::new(args)).unwrap();

    // Get a usable AWS configuration objects for the local environment
    let (sns_config, sqs_config) = match CLI_ARGS.get().unwrap().aws_client_configs().await {
        Ok((sns, sqs)) => (sns, sqs),
        Err(error) => {
            println!(
                "\n\n{:#?}\n\nCould not create usable AWS configuration due to ^\n\n",
                error
            );
            return ExitCode::from(3);
        }
    };

    // Use the inferred AWS config to create SNS and SQS clients
    let sns_client: SNSClient = SNSClient::from_conf(sns_config);
    let sqs_client: SQSClient = SQSClient::from_conf(sqs_config);

    // Allow the created clients to be accessed globally safely via
    // the `OnceCell`s created as part of pinnothera's initialization
    SNS_CLIENT.set(Arc::new(sns_client)).unwrap();
    SQS_CLIENT.set(Arc::new(sqs_client)).unwrap();

    // Spawn async tasks to apply the parsed queue & topic configurations
    let tasks: Vec<_> = PINN_CONFIG
        .get()
        .unwrap()
        .iter()
        .map(|(queue, queue_config)| {
            tokio::spawn(async move {
                match apply_queue_configuration(queue, queue_config).await {
                    Ok(value) => value,
                    Err(value) => value,
                }
            })
        })
        .collect();

    // Wait for all of the spawned tasks to finish
    let results: Vec<u8> = futures_util::future::join_all(tasks)
        .await
        .iter()
        .map(|result| match result {
            Ok(value) => *value,
            Err(_) => 1 as u8,
        })
        .collect();

    ExitCode::from(results.iter().sum::<u8>())
}

// </editor-fold desc="// Main ...">
