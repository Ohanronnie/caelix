use crate::exception::HttpException;

pub type Result<T> = std::result::Result<T, HttpException>;
