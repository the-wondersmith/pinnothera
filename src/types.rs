// Pinnothera's internal structs and enums

// Standard Library Imports
use std::collections::BTreeMap;

// Third Party Imports
use easy_error::{bail, Terminator};
use k8s_openapi::api::core::v1::ConfigMap;
use kube::{api::Api as K8sAPI, Client as K8sClient};
use serde::Deserialize;

// <editor-fold desc="// Type Aliases ...">

pub(crate) type SNSTopicARN = String;
pub(crate) type SQSQueueARN = String;
pub(crate) type SQSQueueURL = String;
pub(crate) type SQSQueueName = String;

pub(crate) type ParsedPinnConfig = BTreeMap<SQSQueueName, SQSQueueConfig>;

// </editor-fold desc="// Type Aliases ...">

// <editor-fold desc="// EnvName enum ...">

#[derive(Eq, Copy, Clone, Debug, PartialEq)]
pub(crate) enum EnvName {
    QA,
    QE,
    Dev,
    Prod,
    Test,
    Local,
    Preview,
    Unknown,
}

impl EnvName {
    pub fn from<T: AsRef<str>>(value: Option<T>) -> EnvName {
        if value.is_none() {
            return EnvName::Unknown;
        }

        match value.unwrap().as_ref().to_uppercase().as_str() {
            "QE" => EnvName::QE,
            "Q" | "QA" => EnvName::QA,
            "L" | "LOCAL" => EnvName::Local,
            "PREVIEW" => EnvName::Preview,
            "T" | "TEST" | "TESTING" => EnvName::Test,
            "D" | "DEV" | "DEVELOPMENT" => EnvName::Dev,
            "P" | "PROD" | "PRODUCTION" => EnvName::Prod,
            _ => EnvName::Unknown,
        }
    }

    pub fn as_suffix(&self) -> &str {
        match self {
            EnvName::QA => "qa",
            EnvName::QE => "qe",
            EnvName::Dev => "dev",
            EnvName::Prod => "prod",
            EnvName::Test => "test",
            EnvName::Local => "local",
            EnvName::Preview => "preview",
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

impl PinnConfig {
    #[allow(dead_code)]
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

    pub async fn from_cluster<T: AsRef<str>>(
        client: K8sClient,
        env_name: &Option<T>,
        namespace: &Option<T>,
        configmap_name: &T,
    ) -> Result<(EnvName, PinnConfig), Terminator> {
        // Ensure the name of the target configmap is usable
        let configmap_name: &str = configmap_name.as_ref();

        // Read `ConfigMap`s in the specified (or default) namespace
        // into the typed interface from k8s-openapi
        let (config_maps, namespace): (K8sAPI<ConfigMap>, String) = if let Some(value) = &namespace
        {
            (
                K8sAPI::namespaced(client, value.as_ref()),
                format!("cluster namespace '{}'", value.as_ref()),
            )
        } else {
            (
                K8sAPI::default_namespaced(client),
                "the current cluster namespace".to_string(),
            )
        };

        // Use the typed interface to pull the namespace's
        // pinnothera configuration (if it exists)
        let pinn_confmap: ConfigMap = match config_maps.get_opt(configmap_name).await? {
            Some(obj) => obj,
            None => {
                println!(
                    "No `ConfigMap` named '{}' in {}!",
                    configmap_name, &namespace
                );
                bail!("")
            }
        };

        // Pull out the ConfigMap's `annotations` element (if it exists)
        let annotations: BTreeMap<String, String> = match pinn_confmap.metadata.annotations {
            Some(obj) => obj,
            None => BTreeMap::new(),
        };

        let env_name: EnvName = match env_name {
            Some(value) => EnvName::from(Some(value)),
            None => EnvName::from(annotations.get("app-env")),
        };

        // Pull out the ConfigMap's `data` element (if it exists)
        let confs_map: BTreeMap<String, String> = match pinn_confmap.data {
            Some(obj) => obj,
            None => {
                println!(
                    "The '{}' `ConfigMap` retrieved from {} has no `data` element!",
                    configmap_name, &namespace,
                );
                bail!("")
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

        println!("The `data` element in the '{}' ConfigMap retrieved from {} has no pinnothera-recognized keys!", configmap_name, &namespace, );

        bail!("")
    }
}

// </editor-fold desc="// PinnConfig struct ...">
