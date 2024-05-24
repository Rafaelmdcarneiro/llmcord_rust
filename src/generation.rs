use std::{collections::HashSet, thread::JoinHandle};

use rand::SeedableRng;
use serenity::model::prelude::MessageId;
use thiserror::Error;

#[derive(Debug, Error, Clone)]
pub enum InferenceError {
    #[error("The generation was cancelled.")]
    Cancelled,
    #[error("{0}")]
    Custom(String),
}
impl InferenceError {
    pub fn custom(s: impl Into<String>) -> Self {
        Self::Custom(s.into())
    }
}

pub struct Request {
    pub prompt: String,
    pub batch_size: usize,
    pub token_tx: flume::Sender<Token>,
    pub message_id: MessageId,
    pub seed: Option<u64>,
}

pub enum Token {
    Token(String),
    Error(InferenceError),
}

pub fn make_thread(
    model: Box<dyn llm::Model>,
    request_rx: flume::Receiver<Request>,
    cancel_rx: flume::Receiver<MessageId>,
) -> JoinHandle<()> {
    std::thread::spawn(move || loop {
        if let Ok(request) = request_rx.try_recv() {
            match process_incoming_request(&request, model.as_ref(), &cancel_rx) {
                Ok(_) => {}
                Err(e) => {
                    if let Err(err) = request.token_tx.send(Token::Error(e)) {
                        eprintln!("Failed to send error: {err:?}");
                    }
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(5));
    })
}

fn process_incoming_request(
    request: &Request,
    model: &dyn llm::Model,
    cancel_rx: &flume::Receiver<MessageId>,
) -> Result<(), InferenceError> {
    let mut rng = if let Some(seed) = request.seed {
        rand::rngs::StdRng::seed_from_u64(seed)
    } else {
        rand::rngs::StdRng::from_entropy()
    };

    let mut session = model.start_session(Default::default());

    let params = llm::InferenceParameters {
        sampler: llm::samplers::default_samplers(),
    };

    session
        .infer(
            model,
            &mut rng,
            &llm::InferenceRequest {
                prompt: (&request.prompt).into(),
                parameters: &params,
                play_back_previous_tokens: false,
                maximum_token_count: None,
            },
            &mut Default::default(),
            move |t| {
                let cancellation_requests: HashSet<_> = cancel_rx.drain().collect();
                if cancellation_requests.contains(&request.message_id) {
                    return Err(InferenceError::Cancelled);
                }

                match t {
                    llm::InferenceResponse::SnapshotToken(t)
                    | llm::InferenceResponse::PromptToken(t)
                    | llm::InferenceResponse::InferredToken(t) => request
                        .token_tx
                        .send(Token::Token(t))
                        .map_err(|_| InferenceError::custom("Failed to send token to channel."))?,
                    llm::InferenceResponse::EotToken => {}
                }

                Ok(llm::InferenceFeedback::Continue)
            },
        )
        .map(|_| ())
        .map_err(|e| match e {
            llm::InferenceError::UserCallback(e) => {
                e.downcast::<InferenceError>().unwrap().as_ref().clone()
            }
            e => InferenceError::custom(e.to_string()),
        })
}
