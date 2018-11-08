use bytes::Bytes;
use h2;
use http::{self, HeaderMap};
use http::header::{HeaderValue, InvalidHeaderValueBytes};
use std::io;

#[derive(Debug, Clone)]
pub struct Status {
    code: Code,
    error_message: Option<Bytes>,
    binary_error_details: Option<Bytes>,
}

const GRPC_STATUS_HEADER_CODE: &str = "grpc-status";
const GRPC_STATUS_MESSAGE_HEADER: &str = "grpc-message";
const GRPC_STATUS_DETAILS_HEADER: &str = "grpc-status-details-bin";

impl Status {
    pub fn with_code(code: Code) -> Status {
        Status {
            code,
            error_message: None,
            binary_error_details: None,
        }
    }

    pub fn from_header_map(header_map: &HeaderMap) -> Option<Status> {
        header_map.get(GRPC_STATUS_HEADER_CODE).clone().map(|code| {
            let code = ::Code::from_bytes(code.as_ref());
            let error_message = header_map.get(GRPC_STATUS_HEADER_CODE)
                .map(|h| Bytes::from(h.as_bytes()));
            let binary_error_details = header_map.get(GRPC_STATUS_DETAILS_HEADER)
                .map(|h| Bytes::from(h.as_bytes()));
            Status {
                code,
                error_message,
                binary_error_details,
            }
        })
    }

    pub fn code(&self) -> Code {
        self.code
    }

    pub fn error_message(&self) -> &Option<Bytes> {
        &self.error_message
    }

    pub fn binary_error_details(&self) -> &Option<Bytes> {
        &self.binary_error_details
    }

    // TODO: It would be nice for this not to be public
    pub fn to_header_map(&self) -> Result<HeaderMap, h2::Error> {
        let mut header_map = HeaderMap::with_capacity(3);

        let status_header = match self.code {
            Code::Ok => HeaderValue::from_static("0"),
            Code::Canceled => HeaderValue::from_static("1"),
            Code::Unknown => HeaderValue::from_static("2"),
            Code::InvalidArgument => HeaderValue::from_static("3"),
            Code::DeadlineExceeded => HeaderValue::from_static("4"),
            Code::NotFound => HeaderValue::from_static("5"),
            Code::AlreadyExists => HeaderValue::from_static("6"),
            Code::PermissionDenied => HeaderValue::from_static("7"),
            Code::ResourceExhausted => HeaderValue::from_static("8"),
            Code::FailedPrecondition => HeaderValue::from_static("9"),
            Code::Aborted => HeaderValue::from_static("10"),
            Code::OutOfRange => HeaderValue::from_static("11"),
            Code::Unimplemented => HeaderValue::from_static("12"),
            Code::Internal => HeaderValue::from_static("13"),
            Code::Unavailable => HeaderValue::from_static("14"),
            Code::DataLoss => HeaderValue::from_static("15"),
            Code::Unauthenticated => HeaderValue::from_static("16"),
        };
        header_map.insert(GRPC_STATUS_HEADER_CODE, status_header);
        if let Some(ref message) = self.error_message {
            header_map.insert(GRPC_STATUS_MESSAGE_HEADER, HeaderValue::from_shared(message.clone())
                .map_err(invalid_header_value_byte_to_h2)?);
        }
        if let Some(ref details) = self.binary_error_details {
            header_map.insert(GRPC_STATUS_DETAILS_HEADER, HeaderValue::from_shared(details.clone())
                .map_err(invalid_header_value_byte_to_h2)?);
        }
        Ok(header_map)
    }
}

fn invalid_header_value_byte_to_h2(err: InvalidHeaderValueBytes) -> h2::Error {
    debug!("Invalid header: {:?}", err);
    io::Error::new(io::ErrorKind::InvalidData, "Couldn't serialize non-text status header").into()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    Ok = 0,
    Canceled = 1,
    Unknown = 2,
    InvalidArgument = 3,
    DeadlineExceeded = 4,
    NotFound = 5,
    AlreadyExists = 6,
    PermissionDenied = 7,
    ResourceExhausted = 8,
    FailedPrecondition = 9,
    Aborted = 10,
    OutOfRange = 11,
    Unimplemented = 12,
    Internal = 13,
    Unavailable = 14,
    DataLoss = 15,
    Unauthenticated = 16,
}

impl Code {
    pub(crate) fn from_bytes(bytes: &[u8]) -> Code {
        match bytes.len() {
            1 => {
                match bytes[0] {
                    b'0' => Code::Ok,
                    b'1' => Code::Canceled,
                    b'2' => Code::Unknown,
                    b'3' => Code::InvalidArgument,
                    b'4' => Code::DeadlineExceeded,
                    b'5' => Code::NotFound,
                    b'6' => Code::AlreadyExists,
                    b'7' => Code::PermissionDenied,
                    b'8' => Code::ResourceExhausted,
                    b'9' => Code::FailedPrecondition,
                    _ => Code::parse_err(),
                }
            },
            2 => {
                match (bytes[0], bytes[1]) {
                    (b'1', b'0') => Code::Aborted,
                    (b'1', b'1') => Code::OutOfRange,
                    (b'1', b'2') => Code::Unimplemented,
                    (b'1', b'3') => Code::Internal,
                    (b'1', b'4') => Code::Unavailable,
                    (b'1', b'5') => Code::DataLoss,
                    (b'1', b'6') => Code::Unauthenticated,
                    _ => Code::parse_err(),
                }
            },
            _ => Code::parse_err(),
        }
    }

    fn parse_err() -> Code {
        trace!("error parsing grpc-status");
        Code::Unknown
    }
}

impl From<h2::Error> for Status {
    fn from(_err: h2::Error) -> Self {
        //TODO: https://grpc.io/docs/guides/wire.html#errors
        Status::with_code(Code::Internal)
    }
}

impl From<Status> for h2::Error {
    fn from(_status: Status) -> Self {
        // TODO: implement
        h2::Reason::INTERNAL_ERROR.into()
    }
}

///
/// Take the `Status` value from `trailers` if it is available, else from `status_code`.
///
pub fn infer_grpc_status(trailers: Option<HeaderMap>, status_code: http::StatusCode) -> Result<(), ::Error> {
    if let Some(trailers) = trailers {
        if let Some(status) = ::Status::from_header_map(&trailers) {
            if status.code() == ::Code::Ok {
                return Ok(());
            } else {
                return Err(::Error::Grpc(status, trailers));
            }
        }
    }
    trace!("trailers missing grpc-status");
    let code = match status_code {
        // Borrowed from https://github.com/grpc/grpc/blob/master/doc/http-grpc-status-mapping.md
        http::StatusCode::BAD_REQUEST => Code::Internal,
        http::StatusCode::UNAUTHORIZED => Code::Unauthenticated,
        http::StatusCode::FORBIDDEN => Code::PermissionDenied,
        http::StatusCode::NOT_FOUND => Code::Unimplemented,
        http::StatusCode::TOO_MANY_REQUESTS |
        http::StatusCode::BAD_GATEWAY |
        http::StatusCode::SERVICE_UNAVAILABLE |
        http::StatusCode::GATEWAY_TIMEOUT => Code::Unavailable,
        _ => Code::Unknown,
    };
    let status = Status::with_code(code);
    let header_map = status.to_header_map().unwrap();
    Err(::Error::Grpc(status, header_map))
}
