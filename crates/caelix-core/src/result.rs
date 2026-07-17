use crate::exception::HttpException;

/// Public Caelix type alias `Result`.
pub type Result<T> = std::result::Result<T, HttpException>;
