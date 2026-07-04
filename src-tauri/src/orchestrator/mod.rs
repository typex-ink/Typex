//! Orchestrator：唯一的业务流程所有者（07 §3）。
//! M0 阶段是最小串联（听写管线直通）；CP-1.1/1.2 替换为状态机 + 执行器。
pub mod pipeline;
pub mod session;

use crate::audio::AudioService;
use crate::error::{ErrorCode, Result, TypexError};
use crate::hotkey::HotkeyEvent;
use crate::inject::InjectorChain;
use crate::providers::stt::{AudioInput, SttOptions, SttProvider};
use crate::settings::SettingsService;
use crate::types::SessionMode;
use std::sync::Arc;
use tokio::sync::mpsc;

/// M0 最小 orchestrator：hotkey 事件 → 录音 → STT → 注入。
/// 无状态机（CP-1.1 加入）、无 HUD 反馈（CP-1.3 加入）。
pub struct Orchestrator {
    pub settings: Arc<SettingsService>,
    pub audio: Arc<AudioService>,
    pub injector: Arc<InjectorChain>,
    pub stt: Arc<dyn SttProvider>,
}

impl Orchestrator {
    /// 消费 hotkey 语义事件的主循环（tokio task）。
    pub async fn run(self: Arc<Self>, mut hotkeys: mpsc::UnboundedReceiver<HotkeyEvent>) {
        let (level_tx, mut level_rx) = mpsc::unbounded_channel::<Vec<f32>>();
        // M0: 电平暂时丢弃（CP-1.3 推给 HUD）
        tokio::spawn(async move { while level_rx.recv().await.is_some() {} });

        let mut recording = false;
        while let Some(ev) = hotkeys.recv().await {
            match ev {
                HotkeyEvent::TriggerDown { mode: SessionMode::Dictation } if !recording => {
                    match self.audio.start("", level_tx.clone()) {
                        Ok(()) => {
                            recording = true;
                            tracing::info!("录音开始");
                        }
                        Err(e) => tracing::error!("录音启动失败: {}", e.message),
                    }
                }
                HotkeyEvent::TriggerUp { held_ms } if recording => {
                    recording = false;
                    if held_ms < 350 {
                        // M0 简化：短按也结束并处理（toggle 语义在 CP-1.1 状态机实现）
                    }
                    let this = self.clone();
                    tokio::spawn(async move {
                        if let Err(e) = this.finish_dictation().await {
                            tracing::error!("听写管线失败: {}", e.message);
                        }
                    });
                }
                HotkeyEvent::Yielded | HotkeyEvent::EscPressed if recording => {
                    recording = false;
                    self.audio.cancel();
                    tracing::info!("录音取消");
                }
                _ => {}
            }
        }
    }

    async fn finish_dictation(&self) -> Result<()> {
        let rec = self.audio.stop()?;
        tracing::info!("录音结束: {} ms, {} bytes wav", rec.duration_ms, rec.wav_16k_mono.len());
        if rec.duration_ms < 300 {
            return Err(TypexError::new(ErrorCode::NoSpeech, "录音过短"));
        }
        let settings = self.settings.get();
        let transcript = self
            .stt
            .transcribe(
                AudioInput { wav_16k_mono: rec.wav_16k_mono, duration_ms: rec.duration_ms },
                SttOptions {
                    language: Some(settings.dictation.language.clone()),
                    prompt: None,
                    temperature: None,
                },
            )
            .await
            .map_err(crate::error::TypexError::from)?;
        let text = transcript.text.trim();
        if text.is_empty() {
            return Err(TypexError::new(ErrorCode::NoSpeech, "没有听到声音"));
        }
        tracing::info!("转写完成: {} 字", text.chars().count());
        self.injector.inject(text)?;
        tracing::info!("注入完成");
        Ok(())
    }
}
