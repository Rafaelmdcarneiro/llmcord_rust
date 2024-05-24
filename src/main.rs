use anyhow::Context as AnyhowContext;
use serenity::{model::prelude::*, Client};

mod config;
mod constant;
mod generation;
mod handler;
mod util;

use config::Configuration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Configuration::load()?;

    let model = llm::load_dynamic(
        config.model.architecture(),
        &config.model.path,
        llm::TokenizerSource::Embedded,
        llm::ModelParameters {
            prefer_mmap: config.model.prefer_mmap,
            context_size: config.model.context_token_length,
            use_gpu: config.model.use_gpu,
            gpu_layers: config.model.gpu_layers,
            ..Default::default()
        },
        llm::load_progress_callback_stdout,
    )?;

    let mut client = Client::builder(
        config
            .authentication
            .discord_token
            .as_deref()
            .context("Expected authentication.discord_token to be filled in config")?,
        GatewayIntents::default(),
    )
    .event_handler(handler::Handler::new(config, model))
    .await
    .context("Error creating client")?;

    if let Err(why) = client.start().await {
        println!("Client error: {why:?}");
    }

    Ok(())
}
