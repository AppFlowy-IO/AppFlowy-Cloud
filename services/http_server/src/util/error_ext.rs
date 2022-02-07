use flowy_collaboration::errors::CollaborateError;
use http_flowy::errors::ServerError;

pub fn into_collaborate_error(error: ServerError) -> CollaborateError {
    if error.is_record_not_found() {
        CollaborateError::record_not_found()
    } else {
        CollaborateError::internal().context(error.msg.clone())
    }
}