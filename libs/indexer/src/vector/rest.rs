use crate::thread_pool::CatchedPanic;

#[derive(Debug, thiserror::Error)]
#[error("{fault}: {kind}")]
pub struct EmbedError {
  pub kind: EmbedErrorKind,
  pub fault: FaultSource,
}

impl EmbedError {
  pub(crate) fn rest_unauthorized(error_response: Option<String>) -> EmbedError {
    Self {
      kind: EmbedErrorKind::RestUnauthorized(error_response),
      fault: FaultSource::User,
    }
  }

  pub(crate) fn rest_too_many_requests(error_response: Option<String>) -> EmbedError {
    Self {
      kind: EmbedErrorKind::RestTooManyRequests(error_response),
      fault: FaultSource::Runtime,
    }
  }

  pub(crate) fn rest_bad_request(error_response: Option<String>) -> EmbedError {
    Self {
      kind: EmbedErrorKind::RestBadRequest(error_response),
      fault: FaultSource::User,
    }
  }

  pub(crate) fn rest_internal_server_error(
    code: u16,
    error_response: Option<String>,
  ) -> EmbedError {
    Self {
      kind: EmbedErrorKind::RestInternalServerError(code, error_response),
      fault: FaultSource::Runtime,
    }
  }

  pub(crate) fn rest_other_status_code(code: u16, error_response: Option<String>) -> EmbedError {
    Self {
      kind: EmbedErrorKind::RestOtherStatusCode(code, error_response),
      fault: FaultSource::Unhandled,
    }
  }

  pub(crate) fn rest_network(transport: ureq::Transport) -> EmbedError {
    Self {
      kind: EmbedErrorKind::RestNetwork(transport),
      fault: FaultSource::Runtime,
    }
  }
}

#[derive(Debug, thiserror::Error)]
pub enum EmbedErrorKind {
  #[error("could not authenticate against {}", option_info(.0.as_deref(), "server replied with "))]
  RestUnauthorized(Option<String>),

  #[error("sent too many requests to embedding server{}", option_info(.0.as_deref(), "server replied with "))]
  RestTooManyRequests(Option<String>),

  #[error("received bad request HTTP from embedding server{}", option_info(.0.as_deref(), "server replied with "))]
  RestBadRequest(Option<String>),

  #[error("received internal error HTTP {0} from embedding server{}", option_info(.1.as_deref(), "server replied with "))]
  RestInternalServerError(u16, Option<String>),

  #[error("received unexpected HTTP {0} from embedding server{}", option_info(.1.as_deref(), "server replied with "))]
  RestOtherStatusCode(u16, Option<String>),

  #[error("could not reach embedding server:\n  - {0}")]
  RestNetwork(ureq::Transport),

  #[error(transparent)]
  PanicInThreadPool(#[from] CatchedPanic),
}

fn option_info(info: Option<&str>, prefix: &str) -> String {
  match info {
    Some(info) => format!("\n  - {prefix}`{info}`"),
    None => String::new(),
  }
}

#[derive(Debug, Clone, Copy)]
pub enum FaultSource {
  User,
  Runtime,
  Unhandled,
}

impl std::fmt::Display for FaultSource {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let s = match self {
      FaultSource::User => "user error",
      FaultSource::Runtime => "runtime error",
      FaultSource::Unhandled => "error",
    };
    f.write_str(s)
  }
}

pub struct Retry {
  pub error: EmbedError,
  strategy: RetryStrategy,
}

pub enum RetryStrategy {
  GiveUp,
  Retry,
  RetryAfterRateLimit,
}

impl Retry {
  pub fn give_up(error: EmbedError) -> Self {
    Self {
      error,
      strategy: RetryStrategy::GiveUp,
    }
  }

  pub fn retry_later(error: EmbedError) -> Self {
    Self {
      error,
      strategy: RetryStrategy::Retry,
    }
  }

  pub fn rate_limited(error: EmbedError) -> Self {
    Self {
      error,
      strategy: RetryStrategy::RetryAfterRateLimit,
    }
  }

  /// Converts a retry strategy into a delay duration based on the number of retry attempts.
  ///
  /// # Retry Strategies
  ///
  /// - **GiveUp**: If the retry strategy is `GiveUp`, the function immediately returns an error,
  ///   indicating that further retries should not be attempted. The error is provided by `self.error`.
  ///   
  /// - **Retry**: If the retry strategy is `Retry`, the function applies exponential backoff based
  ///   on the attempt number. The delay increases exponentially with each retry attempt (e.g.,
  ///   `10^attempt` milliseconds). For the first retry, the delay will be 10 milliseconds,
  ///   for the second retry 100 milliseconds, and so on.
  ///
  /// - **RetryAfterRateLimit**: If the retry strategy is `RetryAfterRateLimit`, the function adds
  ///   a fixed delay of 100 milliseconds to the exponential backoff calculation. For example,
  ///   for the first retry, the delay will be `100 + 10^1 = 110` milliseconds, and for the second
  ///   retry, it will be `100 + 10^2 = 200` milliseconds.
  pub fn into_duration(self, attempt: u32) -> Result<std::time::Duration, Box<EmbedError>> {
    match self.strategy {
      RetryStrategy::GiveUp => Err(Box::new(self.error)),
      RetryStrategy::Retry => Ok(std::time::Duration::from_millis((10u64).pow(attempt))),
      RetryStrategy::RetryAfterRateLimit => {
        Ok(std::time::Duration::from_millis(100 + 10u64.pow(attempt)))
      },
    }
  }
}

#[allow(clippy::result_large_err)]
pub(crate) fn check_ureq_response(
  response: Result<ureq::Response, ureq::Error>,
) -> Result<ureq::Response, Retry> {
  match response {
    Ok(response) => Ok(response),
    Err(ureq::Error::Status(code, response)) => {
      let error_response: Option<String> = response.into_string().ok();
      Err(match code {
        401 => Retry::give_up(EmbedError::rest_unauthorized(error_response)),
        429 => Retry::rate_limited(EmbedError::rest_too_many_requests(error_response)),
        400 => Retry::give_up(EmbedError::rest_bad_request(error_response)),
        500..=599 => {
          Retry::retry_later(EmbedError::rest_internal_server_error(code, error_response))
        },
        402..=499 => Retry::give_up(EmbedError::rest_other_status_code(code, error_response)),
        _ => Retry::retry_later(EmbedError::rest_other_status_code(code, error_response)),
      })
    },
    Err(ureq::Error::Transport(transport)) => {
      Err(Retry::retry_later(EmbedError::rest_network(transport)))
    },
  }
}
