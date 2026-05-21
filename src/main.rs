use std::env;
use std::time::{Duration, Instant};

use anyhow::Result;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use teloxide::prelude::*;
use teloxide::types::MessageId;
use teloxide::RequestError;

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    stream: bool,
}

#[derive(Deserialize)]
struct OllamaStreamChunk {
    #[serde(default)]
    response: String,
    #[serde(default)]
    done: bool,
}

const TELEGRAM_MAX_CHARS: usize = 4000;
const EDIT_INTERVAL: Duration = Duration::from_millis(1200);

fn display_text(text: &str) -> String {
    let count = text.chars().count();
    if count <= TELEGRAM_MAX_CHARS {
        text.to_string()
    } else {
        let skip = count - TELEGRAM_MAX_CHARS;
        let tail: String = text.chars().skip(skip).collect();
        format!("...{}", tail)
    }
}

async fn edit_if_changed(
    bot: &Bot,
    chat_id: ChatId,
    message_id: MessageId,
    text: &str,
    last_sent: &mut String,
) {
    if text.is_empty() || text == last_sent {
        return;
    }
    match bot.edit_message_text(chat_id, message_id, text).await {
        Ok(_) => {
            *last_sent = text.to_string();
        }
        Err(RequestError::Api(err)) => {
            log::warn!("Edit skipped: {:?}", err);
        }
        Err(err) => {
            log::warn!("Edit failed: {:?}", err);
        }
    }
}

async fn stream_ollama_to_telegram(
    bot: &Bot,
    chat_id: ChatId,
    message_id: MessageId,
    client: &Client,
    ollama_url: &str,
    model: &str,
    prompt: &str,
) -> Result<()> {
    let body = OllamaRequest {
        model: model.to_string(),
        prompt: prompt.to_string(),
        stream: true,
    };

    let response = client
        .post(format!("{}/api/generate", ollama_url))
        .json(&body)
        .send()
        .await?
        .error_for_status()?;

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut accumulated = String::new();
    let mut last_sent = String::new();
    let mut last_edit = Instant::now() - EDIT_INTERVAL;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(newline_idx) = buffer.find('\n') {
            let line: String = buffer.drain(..=newline_idx).collect();
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let parsed: OllamaStreamChunk = serde_json::from_str(line)?;
            accumulated.push_str(&parsed.response);

            let should_edit = parsed.done || last_edit.elapsed() >= EDIT_INTERVAL;
            if should_edit {
                let text = display_text(&accumulated);
                edit_if_changed(bot, chat_id, message_id, &text, &mut last_sent).await;
                last_edit = Instant::now();
            }
        }
    }

    let final_text = display_text(&accumulated);
    edit_if_changed(bot, chat_id, message_id, &final_text, &mut last_sent).await;

    Ok(())
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    pretty_env_logger::init();

    log::info!("Starting bot...");

    let bot = Bot::from_env();

    let client = Client::new();

    let ollama_url =
        env::var("OLLAMA_URL").expect("OLLAMA_URL missing");

    let model =
        env::var("MODEL").expect("MODEL missing");

    teloxide::repl(bot, move |bot: Bot, msg: Message| {
        let client = client.clone();
        let ollama_url = ollama_url.clone();
        let model = model.clone();

        async move {
            if let Some(text) = msg.text() {
                let placeholder = bot
                    .send_message(msg.chat.id, "Thinking...")
                    .await?;

                if let Err(err) = stream_ollama_to_telegram(
                    &bot,
                    msg.chat.id,
                    placeholder.id,
                    &client,
                    &ollama_url,
                    &model,
                    text,
                )
                .await
                {
                    log::error!("Ollama error: {:?}", err);

                    bot.edit_message_text(
                        msg.chat.id,
                        placeholder.id,
                        "Failed connect to Ollama",
                    )
                    .await?;
                }
            }

            respond(())
        }
    })
    .await;
}
