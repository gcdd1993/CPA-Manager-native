use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Message(String),
    #[error("文件操作失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("网络请求失败：{0}")]
    Network(#[from] reqwest::Error),
    #[error("数据解析失败：{0}")]
    Json(#[from] serde_json::Error),
    #[error("压缩包无效：{0}")]
    Zip(#[from] zip::result::ZipError),
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

pub fn message(value: impl Into<String>) -> AppError {
    AppError::Message(value.into())
}
