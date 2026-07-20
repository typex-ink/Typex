//! SttProvider trait（03 §2）。
pub mod openai_compat;
pub mod volcengine;

use super::ProviderError;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

/// 内部统一喂 16 kHz mono WAV（03 §2）。
#[derive(Debug, Clone)]
pub struct AudioInput {
    pub wav_16k_mono: Vec<u8>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct SttOptions {
    /// ISO-639-1；None = 自动检测
    pub language: Option<String>,
    /// 术语引导（F-10 个人词典入口）
    pub prompt: Option<String>,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct Transcript {
    pub text: String,
    pub detected_language: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SttCapabilities {
    /// 单次请求最大音频字节数（切片阈值；None = 无上限）
    pub max_bytes: Option<usize>,
    pub supports_prompt: bool,
    pub supports_language: bool,
}

#[async_trait::async_trait]
pub trait SttProvider: Send + Sync {
    async fn transcribe(
        &self,
        audio: AudioInput,
        opts: SttOptions,
    ) -> Result<Transcript, ProviderError>;
    fn capabilities(&self) -> SttCapabilities;

    /// 长录音入口。默认按 capability 自动切片；包装层可覆盖以施加整段调用策略。
    async fn transcribe_auto_chunk(
        &self,
        audio: AudioInput,
        opts: SttOptions,
        vad: crate::settings::schema::VadSettings,
    ) -> Result<Transcript, ProviderError> {
        transcribe_auto_chunk_unbounded(self, audio, opts, vad).await
    }
}

/// 在进入 Tokio blocking pool 前串行化本地 STT 原生任务。
///
/// owned permit 被移入 blocking closure；即使异步等待者被取消，也要等原生推理真正
/// 结束后才允许下一次任务进入，避免超时重试在 blocking pool 中堆积。
#[derive(Clone)]
#[cfg(any(feature = "local-models", test))]
pub(crate) struct NativeSttJobGate {
    permit: Arc<tokio::sync::Semaphore>,
}

#[cfg(any(feature = "local-models", test))]
impl NativeSttJobGate {
    pub(crate) fn new() -> Self {
        Self {
            permit: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }

    pub(crate) async fn run<T, F>(
        &self,
        task_label: &'static str,
        job: F,
    ) -> Result<T, ProviderError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, ProviderError> + Send + 'static,
    {
        let permit = Arc::clone(&self.permit)
            .acquire_owned()
            .await
            .map_err(|_| ProviderError::InvalidRequest("本地 STT 调度已关闭".into()))?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            job()
        })
        .await
        .map_err(|error| ProviderError::InvalidRequest(format!("{task_label}异常: {error}")))?
    }
}

/// 为一个模型服务的所有 STT 调用统一施加 profile 总时限。
///
/// deadline 覆盖完整转写调用，包括远端 adapter 的重试过程。
pub struct TimedSttProvider {
    inner: Arc<dyn SttProvider>,
    timeout: Duration,
}

impl TimedSttProvider {
    pub fn new(inner: Arc<dyn SttProvider>, timeout: Duration) -> Self {
        Self { inner, timeout }
    }
}

async fn run_with_timeout<T, F>(timeout: Duration, operation: F) -> Result<T, ProviderError>
where
    F: Future<Output = Result<T, ProviderError>>,
{
    let deadline = tokio::time::Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| ProviderError::InvalidRequest("模型服务调用超时过大".into()))?;
    match tokio::time::timeout_at(deadline, operation).await {
        Ok(result) => result,
        Err(_) => Err(ProviderError::Timeout),
    }
}

#[async_trait::async_trait]
impl SttProvider for TimedSttProvider {
    async fn transcribe(
        &self,
        audio: AudioInput,
        opts: SttOptions,
    ) -> Result<Transcript, ProviderError> {
        let inner = Arc::clone(&self.inner);
        run_with_timeout(
            self.timeout,
            async move { inner.transcribe(audio, opts).await },
        )
        .await
    }

    fn capabilities(&self) -> SttCapabilities {
        self.inner.capabilities()
    }

    async fn transcribe_auto_chunk(
        &self,
        audio: AudioInput,
        opts: SttOptions,
        vad: crate::settings::schema::VadSettings,
    ) -> Result<Transcript, ProviderError> {
        let inner = Arc::clone(&self.inner);
        run_with_timeout(self.timeout, async move {
            inner.transcribe_auto_chunk(audio, opts, vad).await
        })
        .await
    }
}

const QWEN_ASR_TEXT_MARKER: &str = "<asr_text>";

pub(crate) fn transcript_from_provider_text(
    text: impl AsRef<str>,
    detected_language: Option<String>,
) -> Transcript {
    let (text, marker_language) = strip_qwen_asr_envelope(text.as_ref());
    Transcript {
        text,
        detected_language: detected_language.or(marker_language),
    }
}

fn strip_qwen_asr_envelope(raw: &str) -> (String, Option<String>) {
    let trimmed = raw.trim();
    let Some(marker_pos) = trimmed.find(QWEN_ASR_TEXT_MARKER) else {
        return (trimmed.to_string(), None);
    };

    let prefix = trimmed[..marker_pos].trim();
    let Some(language) = strip_ascii_case_prefix(prefix, "language") else {
        return (trimmed.to_string(), None);
    };
    let language = language
        .trim()
        .trim_start_matches([':', '='])
        .trim()
        .to_string();
    if language.is_empty() || language.contains('<') || language.len() > 64 {
        return (trimmed.to_string(), None);
    }

    let mut body = trimmed[marker_pos + QWEN_ASR_TEXT_MARKER.len()..].trim();
    if let Some(stripped) = body.strip_suffix("</asr_text>") {
        body = stripped.trim();
    }
    (body.to_string(), Some(language))
}

fn strip_ascii_case_prefix<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    match value.get(..prefix.len()) {
        Some(head) if head.eq_ignore_ascii_case(prefix) => Some(&value[prefix.len()..]),
        _ => None,
    }
}

/// 长录音自动切片转写（02 F-1 无时长硬上限）：
/// 超过 provider 单次上限时在 VAD 静音处切片，分段转写后拼接，用户无感。
pub async fn transcribe_auto_chunk(
    provider: &dyn SttProvider,
    audio: AudioInput,
    opts: SttOptions,
    vad: crate::settings::schema::VadSettings,
) -> Result<Transcript, ProviderError> {
    provider.transcribe_auto_chunk(audio, opts, vad).await
}

async fn transcribe_auto_chunk_unbounded<P: SttProvider + ?Sized>(
    provider: &P,
    audio: AudioInput,
    opts: SttOptions,
    vad: crate::settings::schema::VadSettings,
) -> Result<Transcript, ProviderError> {
    let max = provider.capabilities().max_bytes;
    let Some(max_bytes) = max else {
        return provider.transcribe(audio, opts).await;
    };
    if audio.wav_16k_mono.len() <= max_bytes {
        return provider.transcribe(audio, opts).await;
    }

    // 解 WAV → 采样 → 静音处切片
    let reader = hound::WavReader::new(std::io::Cursor::new(&audio.wav_16k_mono))
        .map_err(|e| ProviderError::InvalidRequest(format!("WAV 解析失败: {e}")))?;
    let samples: Vec<f32> = reader
        .into_samples::<i16>()
        .filter_map(|s| s.ok())
        .map(|s| s as f32 / i16::MAX as f32)
        .collect();
    // 16-bit PCM：每采样 2 字节 + 头部余量
    let max_samples = (max_bytes.saturating_sub(1024)) / 2;
    let chunks = crate::audio::vad::split_at_silence(&samples, max_samples, vad);

    let mut full_text = String::new();
    let mut detected = None;
    for (start, end) in chunks {
        let wav = crate::audio::pipeline::to_wav_16k_mono(&samples[start..end], 16_000)
            .map_err(|e| ProviderError::InvalidRequest(e.message))?;
        let duration_ms = ((end - start) as u64 * 1000) / 16_000;
        let t = provider
            .transcribe(
                AudioInput {
                    wav_16k_mono: wav,
                    duration_ms,
                },
                opts.clone(),
            )
            .await?;
        if !full_text.is_empty() && !t.text.is_empty() {
            full_text.push(' ');
        }
        full_text.push_str(t.text.trim());
        detected = detected.or(t.detected_language);
    }
    Ok(Transcript {
        text: full_text,
        detected_language: detected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::schema::{VadMode, VadSettings};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    struct FakeStt {
        response: std::sync::Mutex<Option<Result<Transcript, ProviderError>>>,
        pending: bool,
    }

    impl FakeStt {
        fn responding(response: Result<Transcript, ProviderError>) -> Self {
            Self {
                response: std::sync::Mutex::new(Some(response)),
                pending: false,
            }
        }

        fn pending() -> Self {
            Self {
                response: std::sync::Mutex::new(None),
                pending: true,
            }
        }
    }

    #[async_trait::async_trait]
    impl SttProvider for FakeStt {
        async fn transcribe(
            &self,
            _audio: AudioInput,
            _opts: SttOptions,
        ) -> Result<Transcript, ProviderError> {
            if self.pending {
                return std::future::pending().await;
            }
            self.response.lock().unwrap().take().unwrap()
        }

        fn capabilities(&self) -> SttCapabilities {
            SttCapabilities {
                max_bytes: Some(1234),
                supports_prompt: true,
                supports_language: false,
            }
        }
    }

    struct SlowChunkingStt {
        calls: Arc<AtomicUsize>,
        delay: Duration,
    }

    #[async_trait::async_trait]
    impl SttProvider for SlowChunkingStt {
        async fn transcribe(
            &self,
            _audio: AudioInput,
            _opts: SttOptions,
        ) -> Result<Transcript, ProviderError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            tokio::time::sleep(self.delay).await;
            Ok(Transcript {
                text: format!("chunk-{call}"),
                detected_language: None,
            })
        }

        fn capabilities(&self) -> SttCapabilities {
            SttCapabilities {
                // 16-bit PCM 约 1,600 samples，加 WAV 头部余量。
                max_bytes: Some(1024 + 2 * 1600),
                supports_prompt: false,
                supports_language: false,
            }
        }
    }

    struct BlockingStt {
        native_jobs: NativeSttJobGate,
    }

    #[async_trait::async_trait]
    impl SttProvider for BlockingStt {
        async fn transcribe(
            &self,
            _audio: AudioInput,
            _opts: SttOptions,
        ) -> Result<Transcript, ProviderError> {
            self.native_jobs
                .run("test", || {
                    std::thread::sleep(Duration::from_millis(200));
                    Ok(Transcript {
                        text: "late".into(),
                        detected_language: None,
                    })
                })
                .await
        }

        fn capabilities(&self) -> SttCapabilities {
            SttCapabilities {
                max_bytes: None,
                supports_prompt: false,
                supports_language: false,
            }
        }
    }

    fn audio() -> AudioInput {
        AudioInput {
            wav_16k_mono: Vec::new(),
            duration_ms: 0,
        }
    }

    #[tokio::test]
    async fn profile_timeout_covers_the_complete_transcription() {
        let provider =
            TimedSttProvider::new(Arc::new(FakeStt::pending()), Duration::from_millis(1));

        assert!(matches!(
            provider.transcribe(audio(), SttOptions::default()).await,
            Err(ProviderError::Timeout)
        ));
    }

    #[tokio::test]
    async fn auto_chunk_shares_one_profile_timeout_budget() {
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = TimedSttProvider::new(
            Arc::new(SlowChunkingStt {
                calls: Arc::clone(&calls),
                delay: Duration::from_millis(100),
            }),
            Duration::from_millis(150),
        );
        let samples = vec![0.2; 3200];
        let wav = crate::audio::pipeline::to_wav_16k_mono(&samples, 16_000).unwrap();

        let result = transcribe_auto_chunk(
            &provider,
            AudioInput {
                wav_16k_mono: wav,
                duration_ms: 200,
            },
            SttOptions::default(),
            VadSettings {
                mode: VadMode::Energy,
                ..VadSettings::default()
            },
        )
        .await;

        assert!(matches!(result, Err(ProviderError::Timeout)));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn profile_timeout_is_not_blocked_by_native_local_provider() {
        let provider = TimedSttProvider::new(
            Arc::new(BlockingStt {
                native_jobs: NativeSttJobGate::new(),
            }),
            Duration::from_millis(20),
        );
        let start = Instant::now();

        let result = provider.transcribe(audio(), SttOptions::default()).await;

        assert!(matches!(result, Err(ProviderError::Timeout)));
        assert!(start.elapsed() < Duration::from_millis(150));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancelled_waiter_keeps_native_job_permit_until_work_finishes() {
        let gate = NativeSttJobGate::new();
        let (first_started_tx, first_started_rx) = tokio::sync::oneshot::channel();
        let (release_first_tx, release_first_rx) = std::sync::mpsc::channel();

        let first_gate = gate.clone();
        let first = tokio::spawn(async move {
            first_gate
                .run("first", move || {
                    let _ = first_started_tx.send(());
                    let _ = release_first_rx.recv();
                    Ok(())
                })
                .await
        });
        first_started_rx.await.unwrap();
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());

        let (second_started_tx, mut second_started_rx) = tokio::sync::oneshot::channel();
        let second = tokio::spawn(async move {
            gate.run("second", move || {
                let _ = second_started_tx.send(());
                Ok(())
            })
            .await
        });

        assert!(
            tokio::time::timeout(Duration::from_millis(30), &mut second_started_rx)
                .await
                .is_err()
        );
        release_first_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), &mut second_started_rx)
            .await
            .unwrap()
            .unwrap();
        second.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn profile_timeout_preserves_provider_errors() {
        let provider = TimedSttProvider::new(
            Arc::new(FakeStt::responding(Err(ProviderError::Auth(
                "denied".into(),
            )))),
            Duration::from_secs(60),
        );

        assert!(matches!(
            provider.transcribe(audio(), SttOptions::default()).await,
            Err(ProviderError::Auth(message)) if message == "denied"
        ));
    }

    #[tokio::test]
    async fn profile_timeout_allows_success_and_forwards_capabilities() {
        let provider = TimedSttProvider::new(
            Arc::new(FakeStt::responding(Ok(Transcript {
                text: "done".into(),
                detected_language: Some("en".into()),
            }))),
            Duration::from_secs(60),
        );

        let transcript = provider
            .transcribe(audio(), SttOptions::default())
            .await
            .unwrap();
        assert_eq!(transcript.text, "done");
        assert_eq!(transcript.detected_language.as_deref(), Some("en"));
        let capabilities = provider.capabilities();
        assert_eq!(capabilities.max_bytes, Some(1234));
        assert!(capabilities.supports_prompt);
        assert!(!capabilities.supports_language);
    }

    #[test]
    fn qwen_asr_envelope_is_stripped() {
        let t = transcript_from_provider_text("language Chinese<asr_text>你好。", None);
        assert_eq!(t.text, "你好。");
        assert_eq!(t.detected_language.as_deref(), Some("Chinese"));
    }

    #[test]
    fn qwen_asr_envelope_allows_spacing_and_closing_tag() {
        let t = transcript_from_provider_text(
            " Language: Chinese \n<asr_text>  你好 Typex。 </asr_text> ",
            None,
        );
        assert_eq!(t.text, "你好 Typex。");
        assert_eq!(t.detected_language.as_deref(), Some("Chinese"));
    }

    #[test]
    fn qwen_asr_marker_language_does_not_override_response_language() {
        let t = transcript_from_provider_text(
            "language Chinese<asr_text>你好。",
            Some("zh".to_string()),
        );
        assert_eq!(t.text, "你好。");
        assert_eq!(t.detected_language.as_deref(), Some("zh"));
    }

    #[test]
    fn plain_transcript_is_only_trimmed() {
        let t = transcript_from_provider_text("  language learning <asr_textless>  ", None);
        assert_eq!(t.text, "language learning <asr_textless>");
        assert_eq!(t.detected_language, None);
    }
}
