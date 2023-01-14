mod message;

use axum::routing::{get, post};
use axum::{Json, Router};
use message::slack::WebhookMessage;
use message::PubSubMessage;
use std::{env, net::SocketAddr, str::FromStr};
use tracing::{debug, event_enabled, info, Level};
use tracing_subscriber::{prelude::*, EnvFilter};

#[tokio::main]
async fn main() {
    let env_filter =
        EnvFilter::builder().with_default_directive(Level::INFO.into()).from_env_lossy();

    if env_or_default("JSON_LOG", "false").expect("JSON_LOG should be true or false") {
        tracing::subscriber::set_global_default(
            tracing_subscriber::registry().with(env_filter).with(tracing_stackdriver::layer()),
        )
        .expect("failed to set global default subscriber");
    } else {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
    }

    let listen_addr = SocketAddr::new(
        env_or_default("HOST", "0.0.0.0").expect("LISTEN_HOST should be an IP address"),
        env_or_default("PORT", "8080").expect("LISTEN_PORT should be a number"),
    );
    info!(listen_addr = listen_addr.to_string(), "starting server");

    axum::Server::bind(&listen_addr)
        .serve(
            Router::new()
                .route("/", post(handler))
                .route("/health", get(|| async { "UP" }))
                .into_make_service(),
        )
        .await
        .unwrap()
}

fn env_or_default<F: FromStr>(key: &str, default: &str) -> Result<F, F::Err> {
    env::var(key).unwrap_or_else(|_| default.to_string()).parse()
}

/// The request handler for GKE Cluster Notifications received from Cloud
/// Pub/Sub. Once the message has been deserialized, it will be formatted
/// and logged, then optionally sent to Slack via an Incoming Webhook.
///
/// Currently supports the following event types:
///
///  - type.googleapis.com/google.container.v1beta1.SecurityBulletinEvent
///  - type.googleapis.com/google.container.v1beta1.UpgradeAvailableEvent
///  - type.googleapis.com/google.container.v1beta1.UpgradeEvent
///
/// When the type_url doesn't match a known type, as long as the message can
/// be deserialized, data and type fields will be used to construct a message.
///
async fn handler(Json(psm): Json<PubSubMessage>) -> Result<String, ()> {
    let subscription = psm.subscription;
    let message = match std::env::var("GCP_PROJECT") {
        Ok(project_name) => psm.message.with_project_name(project_name),
        _ => psm.message,
    };
    let formatted = message.fmt();
    let mut slack_message = None::<String>;

    if let Ok(_webhook) = std::env::var("SLACK_WEBHOOK") {
        // UpgradeAvailableEvent messages will be sent for every node pool on a cluster
        // creating quite the flood of messages and so should never be posted to Slack.
        if !message.attributes.is_node_pool_upgrade_available_event() {
            slack_message =
                Some(serde_json::to_string::<WebhookMessage>(&(&message).into()).unwrap());

            // TODO Implement posting JSON message to Slack webhook
            // When unset, will post to default channel on Incoming Webhook configuration
            // "SLACK_CHANNEL" env var determines which channel the message sends to

            // Include webhook message in log entry when debug logging is enabled

            // Post formatted message to Slack webhook
            // "SLACK_WEBHOOK" env var is the URL of the Incoming Webhook
        }
    }

    if event_enabled!(Level::DEBUG) {
        debug!(msg = format_args!("{:#?}", message), subscription, slack_message, "{formatted}");
    } else {
        info!("{formatted}");
    }

    Ok(formatted)
}
