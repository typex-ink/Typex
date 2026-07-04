//! tracing 初始化（07 §5.5）。
//! redact 纪律：日志只记文本长度与耗时，不记转写内容；密钥字段永不入日志
//! （代码 review 层面保证——providers 与 orchestrator 的日志语句均不含内容与凭据）。

use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub fn init(log_dir: Option<std::path::PathBuf>) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let registry = tracing_subscriber::registry().with(filter);
    match log_dir {
        Some(dir) => {
            let appender = tracing_appender::rolling::daily(dir, "typex.log");
            registry
                .with(fmt::layer().with_writer(appender).with_ansi(false))
                .init();
        }
        None => {
            registry.with(fmt::layer()).init();
        }
    }
}
