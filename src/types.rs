// Pinnothera's internal structs and enums

// Standard Library
use std::collections::BTreeMap;

// Third Party
use easy_error::Terminator;
use k8s_openapi::api::core::v1::ConfigMap;
use kube::{api::Api, Client as K8sClient};
use serde::Deserialize;

// <editor-fold desc="// Type Aliases ...">

pub(crate) type SNSTopicARN = String;
pub(crate) type SQSQueueARN = String;
pub(crate) type SQSQueueURL = String;
pub(crate) type SQSQueueName = String;
pub(crate) type SNSSubscriptionARN = String;

pub(crate) type ParsedPinnConfig = BTreeMap<SQSQueueName, SQSQueueConfig>;

// </editor-fold desc="// Type Aliases ...">

// <editor-fold desc="// EnvName enum ...">

#[derive(Eq, Copy, Clone, Debug, PartialEq)]
pub(crate) enum EnvName {
    QA,
    Dev,
    Prod,
    Local,
    Unknown,
}

impl EnvName {
    pub fn from<T: AsRef<str>>(value: Option<T>) -> EnvName {
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

    pub fn for_queue(&self) -> &str {
        match self {
            EnvName::QA => "qa",
            EnvName::Local => "local",
            EnvName::Dev => "development",
            EnvName::Prod => "production",
            EnvName::Unknown => "unknown",
        }
    }

    pub fn for_topic(&self) -> &str {
        match self {
            EnvName::QA => "qa",
            EnvName::Dev => "dev",
            EnvName::Prod => "prod",
            EnvName::Local => "local",
            EnvName::Unknown => "unknown",
        }
    }

    pub fn is_local(&self) -> bool {
        *self == EnvName::Local
    }

    pub fn is_unknown(&self) -> bool {
        *self == EnvName::Unknown
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

// </editor-fold desc="// EnvName ...">

// <editor-fold desc="// SQSQueueConfig ...">

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct SQSQueueConfig {
    pub topics: Vec<String>,
}

// </editor-fold desc="// SQSQueueConfig struct ...">

// <editor-fold desc="// PinnConfig ...">

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct PinnConfig(ParsedPinnConfig);

impl std::ops::Deref for PinnConfig {
    type Target = ParsedPinnConfig;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// impl<T: AsRef<str>> TryFrom<T> for PinnConfig {
//     type Error = Terminator;
//
//     fn try_from(data: T) -> Result<Self, Self::Error> {
//         let data: &str = data.as_ref().trim();
//
//         match data.chars().next()? {
//             // If the leading character is "{"
//             // we've most likely got JSON data
//             '{' => PinnConfig::from_json(data),
//
//             // Otherwise, assume the data is
//             // either improperly formatted, or
//             // actually is YAML data
//             _ => PinnConfig::from_yaml(data),
//         }
//     }
// }

impl PinnConfig {
    pub fn for_unknown_env() -> Result<(EnvName, PinnConfig), Terminator> {
        Ok((EnvName::Unknown, Self::default()))
    }

    pub fn from_json<T: AsRef<str>>(data: T) -> Result<PinnConfig, Terminator> {
        let data: &str = data.as_ref();
        match serde_json::from_str::<PinnConfig>(data) {
            Ok(obj) => Ok(obj),
            Err(error) => {
                println!("Couldn't deserialize JSON data: {}", data);
                Err(error.into())
            }
        }
    }

    pub fn from_yaml<T: AsRef<str>>(data: T) -> Result<PinnConfig, Terminator> {
        let data: &str = data.as_ref();
        match serde_yaml::from_str::<PinnConfig>(data) {
            Ok(obj) => Ok(obj),
            Err(error) => {
                println!("Couldn't deserialize YAML data: {}", data);
                Err(error.into())
            }
        }
    }

    pub async fn from_cluster() -> Result<(EnvName, PinnConfig), Terminator> {
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
                return Self::for_unknown_env();
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
                return Self::for_unknown_env();
            }
        };

        // Parse the data from the first recognized key and return it
        if let Some(data) = confs_map.get("json") {
            return match Self::from_json(data) {
                Ok(config) => Ok((env_name, config)),
                Err(error) => Err(error),
            };
        } else if let Some(data) = confs_map.get("yaml") {
            return match Self::from_yaml(data) {
                Ok(config) => Ok((env_name, config)),
                Err(error) => Err(error),
            };
        };

        println!("The `data` element in the `sns-sqs-config` ConfigMap retrieved from the current cluster namespace has no pinnothera-recognized keys!");

        Self::for_unknown_env()
    }
}

// </editor-fold desc="// PinnConfig struct ...">
