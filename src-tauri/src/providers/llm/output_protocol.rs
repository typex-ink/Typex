//! LLM 输出协议解析器（03 §3.4 / ADR-29）。
//!
//! - 非流式（F-9 文本整理 / F-2 翻译）：`extract_tagged_output` 提取 `<output>...</output>`
//!   外层信封，过滤前言、后记与代码围栏，保留正文内嵌 HTML/XML/代码且不做实体转义。
//! - 流式（F-3a 选区处理）：`SelectionStreamParser` 识别 `<rewrite>...</rewrite>` 与
//!   `<answer>...</answer>` 双显式路由标签；仅明确命中 `<rewrite>` 才允许替换选区，
//!   其余所有输出均立即 Fail-safe 进入回答弹窗流式展示。

use super::prompt::{ANSWER_TAG, REWRITE_TAG};

const MAX_FENCE_LINE_LEN: usize = 32;
const MAX_OPEN_TAG_LEN: usize = 24;
const MAX_CLOSE_TAG_EXTRA_LEN: usize = 16;

/// 非流式信封提取结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedOutput {
    pub text: String,
    pub tag_matched: bool,
}

/// 从完整模型响应中提取 `<tag>...</tag>` 边界信封内容。
///
/// 匹配规则：
/// 1. ASCII 大小写不敏感匹配首个开标签 `<tag>`（允许 `>` 前带 ASCII 空白）。
/// 2. 匹配最后一个闭标签 `</tag>`，避免正文内嵌同名标签（如 HTML `<output>`）被误截断。
/// 3. 若命中开标签但因截断缺失闭标签，取开标签之后全文并剥离末尾残缺局部闭标签。
/// 4. 若未命中开标签，安全回退为 `raw.trim()`，并标记 `tag_matched = false`。
pub fn extract_tagged_output(raw: &str, tag: &str) -> ExtractedOutput {
    let Some((_, content_start)) = find_open_tag(raw, tag) else {
        return ExtractedOutput {
            text: raw.trim().to_string(),
            tag_matched: false,
        };
    };

    let after_open = &raw[content_start..];
    let body = match rfind_close_tag(after_open, tag) {
        Some((close_start, _)) => &after_open[..close_start],
        None => strip_partial_close_tag_suffix(after_open, tag),
    };

    ExtractedOutput {
        text: body.trim().to_string(),
        tag_matched: true,
    }
}

/// F-3a 选区处理路由结论（非敏感日志分类用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionRoute {
    Rewrite,
    Answer,
    FallbackAnswer,
}

/// 流式消费单个 chunk 后的动作。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamStep {
    /// 首次确认需呼出回答弹窗。
    pub open_panel: bool,
    /// 本步可推送到回答弹窗的增量文本（Rewrite 路径恒为 None）。
    pub delta: Option<String>,
    /// 本步刚确定的路由（仅状态跃迁当步为 Some）。
    pub decided_route: Option<SelectionRoute>,
}

/// 流结束后的最终结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionFinish {
    /// 明确命中 `<rewrite>` 且提取正文非空：静默替换选区。
    Rewrite(String),
    /// 回答型（含 Fail-safe 兜底）：推送尾部残余并完成弹窗展示。
    Answer {
        open_panel: bool,
        tail_delta: Option<String>,
        full_text: String,
    },
    /// 最终有效文本为空（例如空标签或纯空白）。
    Empty,
}

enum ParserState {
    Sniffing { buf: String },
    RewriteCollecting { content: String },
    AnswerPendingOpen { pending: String },
    AnswerStreaming { pending: String },
    AnswerClosed,
    FallbackStreaming { waiting_visible: bool },
}

/// F-3a 选区处理流式路由状态机（ADR-29）。
pub struct SelectionStreamParser {
    state: ParserState,
    panel_opened: bool,
    full_answer: String,
}

impl Default for SelectionStreamParser {
    fn default() -> Self {
        Self::new()
    }
}

impl SelectionStreamParser {
    pub fn new() -> Self {
        Self {
            state: ParserState::Sniffing { buf: String::new() },
            panel_opened: false,
            full_answer: String::new(),
        }
    }

    pub fn push(&mut self, chunk: &str) -> StreamStep {
        if chunk.is_empty() {
            return StreamStep::default();
        }
        match &mut self.state {
            ParserState::Sniffing { .. } => self.push_sniffing(chunk),
            ParserState::RewriteCollecting { content } => {
                content.push_str(chunk);
                StreamStep::default()
            }
            ParserState::AnswerPendingOpen { .. } => self.push_answer_pending(chunk),
            ParserState::AnswerStreaming { .. } => self.push_answer_streaming(chunk),
            ParserState::AnswerClosed => StreamStep::default(),
            ParserState::FallbackStreaming { waiting_visible } => {
                if *waiting_visible {
                    let trimmed = chunk.trim_start();
                    if trimmed.is_empty() {
                        return StreamStep::default();
                    }
                    *waiting_visible = false;
                    self.panel_opened = true;
                    self.full_answer.push_str(trimmed);
                    StreamStep {
                        open_panel: true,
                        delta: Some(trimmed.to_string()),
                        decided_route: None,
                    }
                } else {
                    self.full_answer.push_str(chunk);
                    StreamStep {
                        open_panel: false,
                        delta: Some(chunk.to_string()),
                        decided_route: None,
                    }
                }
            }
        }
    }

    pub fn finish(mut self) -> SelectionFinish {
        match self.state {
            ParserState::Sniffing { buf } => {
                // 流结束仍未凑齐 `<rewrite>` / `<answer>`：Fail-safe 当作回答展示，绝不误替换选区
                let fallback = strip_legacy_answer_prefix(&buf).trim().to_string();
                if fallback.is_empty() {
                    SelectionFinish::Empty
                } else {
                    SelectionFinish::Answer {
                        open_panel: !self.panel_opened,
                        tail_delta: Some(fallback.clone()),
                        full_text: fallback,
                    }
                }
            }
            ParserState::RewriteCollecting { content } => {
                let body = match rfind_close_tag(&content, REWRITE_TAG) {
                    Some((close_start, _)) => &content[..close_start],
                    None => strip_partial_close_tag_suffix(&content, REWRITE_TAG),
                };
                let final_text = body.trim().to_string();
                if final_text.is_empty() {
                    SelectionFinish::Empty
                } else {
                    SelectionFinish::Rewrite(final_text)
                }
            }
            ParserState::AnswerPendingOpen { pending } => {
                let body = match find_close_tag(&pending, ANSWER_TAG) {
                    Some((close_start, _)) => &pending[..close_start],
                    None => strip_partial_close_tag_suffix(&pending, ANSWER_TAG),
                };
                let trimmed = body.trim().to_string();
                if trimmed.is_empty() {
                    SelectionFinish::Empty
                } else {
                    SelectionFinish::Answer {
                        open_panel: !self.panel_opened,
                        tail_delta: Some(trimmed.clone()),
                        full_text: trimmed,
                    }
                }
            }
            ParserState::AnswerStreaming { pending } => {
                let stripped = strip_partial_close_tag_suffix(&pending, ANSWER_TAG);
                let tail = if stripped.is_empty() {
                    None
                } else {
                    self.full_answer.push_str(stripped);
                    Some(stripped.to_string())
                };
                let full_text = self.full_answer.trim().to_string();
                if full_text.is_empty() {
                    SelectionFinish::Empty
                } else {
                    SelectionFinish::Answer {
                        open_panel: !self.panel_opened,
                        tail_delta: tail,
                        full_text,
                    }
                }
            }
            ParserState::AnswerClosed | ParserState::FallbackStreaming { .. } => {
                let full_text = self.full_answer.trim().to_string();
                if full_text.is_empty() {
                    SelectionFinish::Empty
                } else {
                    SelectionFinish::Answer {
                        open_panel: false,
                        tail_delta: None,
                        full_text,
                    }
                }
            }
        }
    }

    fn push_sniffing(&mut self, chunk: &str) -> StreamStep {
        let ParserState::Sniffing { buf } = &mut self.state else {
            unreachable!()
        };
        buf.push_str(chunk);

        let trimmed = buf.trim_start();
        if trimmed.is_empty() {
            return StreamStep::default();
        }

        let candidate = match skip_optional_fence_line(trimmed) {
            FenceSkip::WaitMore => return StreamStep::default(),
            FenceSkip::Candidate(rest) => rest,
            FenceSkip::NotFence => trimmed,
        };

        if candidate.is_empty() {
            return StreamStep::default();
        }

        let rewrite_match = parse_open_tag_prefix(candidate, REWRITE_TAG);
        let answer_match = parse_open_tag_prefix(candidate, ANSWER_TAG);

        if let OpenTagMatch::Matched(consumed) = rewrite_match {
            let rest = candidate[consumed..].to_string();
            self.state = ParserState::RewriteCollecting { content: rest };
            return StreamStep {
                open_panel: false,
                delta: None,
                decided_route: Some(SelectionRoute::Rewrite),
            };
        }

        if let OpenTagMatch::Matched(consumed) = answer_match {
            let rest = candidate[consumed..].to_string();
            self.state = ParserState::AnswerPendingOpen {
                pending: String::new(),
            };
            let mut step = self.push_answer_pending(&rest);
            step.decided_route = Some(SelectionRoute::Answer);
            return step;
        }

        if matches!(rewrite_match, OpenTagMatch::Partial)
            || matches!(answer_match, OpenTagMatch::Partial)
        {
            return StreamStep::default();
        }

        // 不再可能是 `<rewrite>` 或 `<answer>`：立即 Fail-safe 走回答弹窗，保留完整原始文本
        let raw_buf = std::mem::take(buf);
        let display = strip_legacy_answer_prefix(&raw_buf).to_string();
        if display.is_empty() {
            self.state = ParserState::FallbackStreaming {
                waiting_visible: true,
            };
            return StreamStep {
                open_panel: false,
                delta: None,
                decided_route: Some(SelectionRoute::FallbackAnswer),
            };
        }
        self.state = ParserState::FallbackStreaming {
            waiting_visible: false,
        };
        self.panel_opened = true;
        self.full_answer.push_str(&display);
        StreamStep {
            open_panel: true,
            delta: Some(display),
            decided_route: Some(SelectionRoute::FallbackAnswer),
        }
    }

    fn push_answer_pending(&mut self, chunk: &str) -> StreamStep {
        let ParserState::AnswerPendingOpen { pending } = &mut self.state else {
            unreachable!()
        };
        pending.push_str(chunk);

        if let Some((close_start, _)) = find_close_tag(pending, ANSWER_TAG) {
            let body = pending[..close_start].trim().to_string();
            self.state = ParserState::AnswerClosed;
            if body.is_empty() {
                return StreamStep::default();
            }
            self.panel_opened = true;
            self.full_answer.push_str(&body);
            return StreamStep {
                open_panel: true,
                delta: Some(body),
                decided_route: None,
            };
        }

        let trimmed = pending.trim_start();
        if trimmed.is_empty() {
            pending.clear();
            return StreamStep::default();
        }

        let keep = partial_close_tag_suffix_len(trimmed, ANSWER_TAG);
        let emit_len = trimmed.len().saturating_sub(keep);
        if emit_len == 0 {
            // 目前仅收到类似 `</ans` 的局部闭标签，还无法确认是否存在可见正文
            let retained = trimmed.to_string();
            *pending = retained;
            return StreamStep::default();
        }

        let emit = trimmed[..emit_len].to_string();
        let retained = trimmed[emit_len..].to_string();
        self.state = ParserState::AnswerStreaming { pending: retained };
        self.panel_opened = true;
        self.full_answer.push_str(&emit);
        StreamStep {
            open_panel: true,
            delta: Some(emit),
            decided_route: None,
        }
    }

    fn push_answer_streaming(&mut self, chunk: &str) -> StreamStep {
        let ParserState::AnswerStreaming { pending } = &mut self.state else {
            unreachable!()
        };
        pending.push_str(chunk);

        if let Some((close_start, _)) = find_close_tag(pending, ANSWER_TAG) {
            let emit = pending[..close_start].trim_end().to_string();
            self.state = ParserState::AnswerClosed;
            let delta = if emit.is_empty() {
                None
            } else {
                self.full_answer.push_str(&emit);
                Some(emit)
            };
            return StreamStep {
                open_panel: false,
                delta,
                decided_route: None,
            };
        }

        let keep = partial_close_tag_suffix_len(pending, ANSWER_TAG);
        let emit_len = pending.len().saturating_sub(keep);
        if emit_len == 0 {
            return StreamStep::default();
        }

        let emit = pending[..emit_len].to_string();
        pending.drain(..emit_len);
        self.full_answer.push_str(&emit);
        StreamStep {
            open_panel: false,
            delta: Some(emit),
            decided_route: None,
        }
    }
}

enum OpenTagMatch {
    Matched(usize),
    Partial,
    Mismatch,
}

fn parse_open_tag_prefix(s: &str, tag: &str) -> OpenTagMatch {
    if s.is_empty() {
        return OpenTagMatch::Partial;
    }
    let bytes = s.as_bytes();
    if bytes[0] != b'<' {
        return OpenTagMatch::Mismatch;
    }
    let rest = &bytes[1..];
    let tag_bytes = tag.as_bytes();
    let cmp_len = rest.len().min(tag_bytes.len());
    if !rest[..cmp_len].eq_ignore_ascii_case(&tag_bytes[..cmp_len]) {
        return OpenTagMatch::Mismatch;
    }
    if rest.len() < tag_bytes.len() {
        return OpenTagMatch::Partial;
    }
    let max_extra = MAX_OPEN_TAG_LEN.saturating_sub(1 + tag_bytes.len());
    let after_name = &rest[tag_bytes.len()..];
    for (idx, &b) in after_name.iter().enumerate() {
        if idx >= max_extra {
            return OpenTagMatch::Mismatch;
        }
        if b == b'>' {
            return OpenTagMatch::Matched(1 + tag_bytes.len() + idx + 1);
        }
        if !b.is_ascii_whitespace() {
            return OpenTagMatch::Mismatch;
        }
    }
    if after_name.len() >= max_extra {
        OpenTagMatch::Mismatch
    } else {
        OpenTagMatch::Partial
    }
}

enum FenceSkip<'a> {
    WaitMore,
    Candidate(&'a str),
    NotFence,
}

fn skip_optional_fence_line(trimmed: &str) -> FenceSkip<'_> {
    if trimmed == "`" || trimmed == "``" {
        return FenceSkip::WaitMore;
    }
    if !trimmed.starts_with("```") {
        return FenceSkip::NotFence;
    }
    match trimmed.find('\n') {
        Some(nl) if nl <= MAX_FENCE_LINE_LEN => {
            let info = trimmed[3..nl].trim_end_matches('\r');
            if info.contains('`') {
                FenceSkip::NotFence
            } else {
                FenceSkip::Candidate(trimmed[nl + 1..].trim_start())
            }
        }
        None if trimmed.len() <= MAX_FENCE_LINE_LEN => FenceSkip::WaitMore,
        _ => FenceSkip::NotFence,
    }
}

/// 匹配首个 `<tag>`（ASCII 大小写不敏感，允许 `>` 前带 ASCII 空白）。
fn find_open_tag(haystack: &str, tag: &str) -> Option<(usize, usize)> {
    let bytes = haystack.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<'
            && let Some(end) = match_tag_body(bytes, i + 1, tag.as_bytes())
        {
            return Some((i, end));
        }
        i += 1;
    }
    None
}

/// 匹配首个 `</tag>`（ASCII 大小写不敏感，允许 `>` 前带 ASCII 空白）。
fn find_close_tag(haystack: &str, tag: &str) -> Option<(usize, usize)> {
    let bytes = haystack.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'<'
            && bytes[i + 1] == b'/'
            && let Some(end) = match_tag_body(bytes, i + 2, tag.as_bytes())
        {
            return Some((i, end));
        }
        i += 1;
    }
    None
}

/// 匹配最后一个 `</tag>`（ASCII 大小写不敏感，允许 `>` 前带 ASCII 空白）。
fn rfind_close_tag(haystack: &str, tag: &str) -> Option<(usize, usize)> {
    let bytes = haystack.as_bytes();
    let mut last = None;
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'<'
            && bytes[i + 1] == b'/'
            && let Some(end) = match_tag_body(bytes, i + 2, tag.as_bytes())
        {
            last = Some((i, end));
        }
        i += 1;
    }
    last
}

/// 从标签名起始位置匹配 `tag` + 最多 `MAX_CLOSE_TAG_EXTRA_LEN - 1` 个 ASCII 空白 + `>`。
fn match_tag_body(bytes: &[u8], name_start: usize, tag: &[u8]) -> Option<usize> {
    let name_end = name_start.checked_add(tag.len())?;
    if name_end >= bytes.len() || !bytes[name_start..name_end].eq_ignore_ascii_case(tag) {
        return None;
    }
    let mut pos = name_end;
    let limit = (name_end + MAX_CLOSE_TAG_EXTRA_LEN).min(bytes.len());
    while pos < limit {
        match bytes[pos] {
            b'>' => return Some(pos + 1),
            b if b.is_ascii_whitespace() => pos += 1,
            _ => return None,
        }
    }
    None
}

/// 计算字符串末尾可能是 `</tag>`（不含 `>`）的局部闭标签前缀长度。
fn partial_close_tag_suffix_len(value: &str, tag: &str) -> usize {
    let bytes = value.as_bytes();
    let max_len = tag.len() + MAX_CLOSE_TAG_EXTRA_LEN + 1;
    let search_start = bytes.len().saturating_sub(max_len);
    for pos in search_start..bytes.len() {
        if bytes[pos] == b'<' && is_partial_close_tag(&bytes[pos..], tag.as_bytes()) {
            return bytes.len() - pos;
        }
    }
    0
}

fn is_partial_close_tag(tail: &[u8], tag: &[u8]) -> bool {
    if tail.is_empty() || tail[0] != b'<' {
        return false;
    }
    if tail.len() == 1 {
        return true;
    }
    if tail[1] != b'/' {
        return false;
    }
    let after_slash = &tail[2..];
    let cmp_len = after_slash.len().min(tag.len());
    if !after_slash[..cmp_len].eq_ignore_ascii_case(&tag[..cmp_len]) {
        return false;
    }
    if after_slash.len() <= tag.len() {
        return true;
    }
    let extra = &after_slash[tag.len()..];
    extra.len() < MAX_CLOSE_TAG_EXTRA_LEN && extra.iter().all(u8::is_ascii_whitespace)
}

/// 截断输出末尾残缺闭标签（至少含 `</` 才剥离，不误伤正文末尾的单个 `<`）。
fn strip_partial_close_tag_suffix<'a>(value: &'a str, tag: &str) -> &'a str {
    let keep = partial_close_tag_suffix_len(value, tag);
    if keep >= 2 {
        &value[..value.len() - keep]
    } else {
        value
    }
}

/// Fallback 分支中温和剥离旧版 `ANSWER:` / `ANSWER：` / `**ANSWER:**` 前缀。
fn strip_legacy_answer_prefix(raw: &str) -> &str {
    let trimmed = raw.trim_start();
    const LEGACY_PREFIXES: &[&str] = &["**ANSWER:**", "**ANSWER：**", "ANSWER:", "ANSWER："];
    for prefix in LEGACY_PREFIXES {
        if trimmed.len() >= prefix.len()
            && trimmed.is_char_boundary(prefix.len())
            && trimmed[..prefix.len()].eq_ignore_ascii_case(prefix)
        {
            return trimmed[prefix.len()..].trim_start();
        }
    }
    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_output_handles_clean_and_wrapped_responses() {
        let clean = extract_tagged_output("<output>今天的会议改到下午三点。</output>", "output");
        assert_eq!(
            clean,
            ExtractedOutput {
                text: "今天的会议改到下午三点。".into(),
                tag_matched: true,
            }
        );

        let noisy = extract_tagged_output(
            "好的，这是整理后的结果：\n```xml\n<OUTPUT>\n第一项\n第二项\n</OUTPUT>\n```\n希望能帮到你。",
            "output",
        );
        assert_eq!(
            noisy,
            ExtractedOutput {
                text: "第一项\n第二项".into(),
                tag_matched: true,
            }
        );
    }

    #[test]
    fn extract_output_preserves_nested_same_tag_and_raw_html_symbols() {
        let nested = extract_tagged_output(
            "前言\n<output>HTML 的 <output> 元素和 <strong>重要</strong> 且 a < b & c > d</output>\n后记",
            "output",
        );
        assert_eq!(
            nested,
            ExtractedOutput {
                text: "HTML 的 <output> 元素和 <strong>重要</strong> 且 a < b & c > d".into(),
                tag_matched: true,
            }
        );
    }

    #[test]
    fn extract_output_handles_empty_truncated_and_missing_tags() {
        let empty = extract_tagged_output("<output>   </output>", "output");
        assert_eq!(
            empty,
            ExtractedOutput {
                text: String::new(),
                tag_matched: true,
            }
        );

        let truncated = extract_tagged_output("<output>截断前正文</out", "output");
        assert_eq!(
            truncated,
            ExtractedOutput {
                text: "截断前正文".into(),
                tag_matched: true,
            }
        );

        let fallback = extract_tagged_output("  直接输出的译文  ", "output");
        assert_eq!(
            fallback,
            ExtractedOutput {
                text: "直接输出的译文".into(),
                tag_matched: false,
            }
        );
    }

    #[test]
    fn selection_parser_collects_rewrite_silently_and_strips_outer_noise() {
        let mut parser = SelectionStreamParser::new();
        assert_eq!(parser.push("```xml\n<rew"), StreamStep::default());
        let step = parser.push("rite>\n改写后的");
        assert_eq!(step.decided_route, Some(SelectionRoute::Rewrite));
        assert!(!step.open_panel);
        assert_eq!(step.delta, None);

        assert_eq!(
            parser.push("文本 <config> & ok</rewrite>\n```\n尾部废话"),
            StreamStep::default()
        );
        assert_eq!(
            parser.finish(),
            SelectionFinish::Rewrite("改写后的文本 <config> & ok".into())
        );
    }

    #[test]
    fn selection_parser_streams_answer_and_strips_split_close_tag_and_trailing_noise() {
        let mut parser = SelectionStreamParser::new();
        assert_eq!(parser.push("  <ANS"), StreamStep::default());
        let open_step = parser.push("WER>\n  ");
        assert_eq!(open_step.decided_route, Some(SelectionRoute::Answer));
        assert!(!open_step.open_panel, "前导空白尚未出现正文时不呼出弹窗");
        assert_eq!(open_step.delta, None);

        let first = parser.push("原因是超时</ans");
        assert!(first.open_panel);
        assert_eq!(first.delta.as_deref(), Some("原因是超时"));

        let close = parser.push("wer>\n希望对你有帮助");
        assert!(!close.open_panel);
        assert_eq!(close.delta, None);

        let ignored = parser.push("，还有什么问题吗？");
        assert_eq!(ignored, StreamStep::default());

        assert_eq!(
            parser.finish(),
            SelectionFinish::Answer {
                open_panel: false,
                tail_delta: None,
                full_text: "原因是超时".into(),
            }
        );
    }

    #[test]
    fn selection_parser_fails_safe_to_answer_for_untagged_and_code_blocks() {
        let mut plain = SelectionStreamParser::new();
        let step = plain.push("这段代码会导致空指针异常。");
        assert!(step.open_panel);
        assert_eq!(step.decided_route, Some(SelectionRoute::FallbackAnswer));
        assert_eq!(step.delta.as_deref(), Some("这段代码会导致空指针异常。"));
        assert_eq!(
            plain.finish(),
            SelectionFinish::Answer {
                open_panel: false,
                tail_delta: None,
                full_text: "这段代码会导致空指针异常。".into(),
            }
        );

        let mut code = SelectionStreamParser::new();
        assert_eq!(code.push("```rust\n"), StreamStep::default());
        let step = code.push("fn main() {}\n```");
        assert!(step.open_panel);
        assert_eq!(step.decided_route, Some(SelectionRoute::FallbackAnswer));
        assert_eq!(step.delta.as_deref(), Some("```rust\nfn main() {}\n```"));
        assert_eq!(
            code.finish(),
            SelectionFinish::Answer {
                open_panel: false,
                tail_delta: None,
                full_text: "```rust\nfn main() {}\n```".into(),
            }
        );
    }

    #[test]
    fn selection_parser_strips_legacy_answer_prefix_in_fallback_and_handles_short_or_empty() {
        for raw in [
            "ANSWER: 旧协议回答",
            "ANSWER：全角冒号回答",
            "**ANSWER:** 加粗回答",
        ] {
            let mut parser = SelectionStreamParser::new();
            let step = parser.push(raw);
            assert!(step.open_panel);
            assert_eq!(step.decided_route, Some(SelectionRoute::FallbackAnswer));
            let text = step.delta.unwrap();
            assert!(!text.contains("ANSWER"));
        }

        // 超短非闭合前缀（如 "<re"）绝不误判为改写
        let mut short = SelectionStreamParser::new();
        assert_eq!(short.push("<re"), StreamStep::default());
        assert_eq!(
            short.finish(),
            SelectionFinish::Answer {
                open_panel: true,
                tail_delta: Some("<re".into()),
                full_text: "<re".into(),
            }
        );

        // 空标签返回 Empty，不替换也不弹窗
        let mut empty_rewrite = SelectionStreamParser::new();
        empty_rewrite.push("<rewrite>  </rewrite>");
        assert_eq!(empty_rewrite.finish(), SelectionFinish::Empty);

        let mut empty_answer = SelectionStreamParser::new();
        assert!(!empty_answer.push("<answer>  </ans").open_panel);
        assert!(!empty_answer.push("wer>").open_panel);
        assert_eq!(empty_answer.finish(), SelectionFinish::Empty);
    }

    #[test]
    fn selection_parser_handles_utf8_before_partial_close_tag() {
        let mut parser = SelectionStreamParser::new();
        let step = parser.push("<answer>中文正文</an");
        assert!(step.open_panel);
        assert_eq!(step.delta.as_deref(), Some("中文正文"));

        let next = parser.push("other> 继续");
        assert_eq!(next.delta.as_deref(), Some("</another> 继续"));

        assert_eq!(
            parser.finish(),
            SelectionFinish::Answer {
                open_panel: false,
                tail_delta: None,
                full_text: "中文正文</another> 继续".into(),
            }
        );
    }

    #[test]
    fn selection_parser_handles_single_long_chunk_rewrite_and_answer() {
        let mut rewrite = SelectionStreamParser::new();
        let long_rewrite = "<rewrite>这是一段远超二十四个字节的单块改写正文，包含 <strong>HTML</strong> 与 URL https://typex.ink。</rewrite>";
        assert!(long_rewrite.len() > MAX_OPEN_TAG_LEN * 2);
        let step = rewrite.push(long_rewrite);
        assert_eq!(step.decided_route, Some(SelectionRoute::Rewrite));
        assert!(!step.open_panel);
        assert_eq!(step.delta, None);
        assert_eq!(
            rewrite.finish(),
            SelectionFinish::Rewrite(
                "这是一段远超二十四个字节的单块改写正文，包含 <strong>HTML</strong> 与 URL https://typex.ink。".into()
            )
        );

        let mut answer = SelectionStreamParser::new();
        let long_answer = "<answer>\n这是一段远超二十四个字节的单块回答正文，说明报错原因是空指针解引用。\n</answer>\n尾部废话";
        let step = answer.push(long_answer);
        assert_eq!(step.decided_route, Some(SelectionRoute::Answer));
        assert!(step.open_panel);
        assert_eq!(
            step.delta.as_deref(),
            Some("这是一段远超二十四个字节的单块回答正文，说明报错原因是空指针解引用。")
        );
        assert_eq!(
            answer.finish(),
            SelectionFinish::Answer {
                open_panel: false,
                tail_delta: None,
                full_text: "这是一段远超二十四个字节的单块回答正文，说明报错原因是空指针解引用。"
                    .into(),
            }
        );
    }

    #[test]
    fn selection_parser_trims_trailing_newline_before_closing_answer_tag() {
        let mut parser = SelectionStreamParser::new();
        let s1 = parser.push("<answer>\n第一行");
        assert!(s1.open_panel);
        assert_eq!(s1.delta.as_deref(), Some("第一行"));

        let s2 = parser.push("\n</answer>\n尾部废话");
        assert_eq!(s2.delta, None);
        assert_eq!(
            parser.finish(),
            SelectionFinish::Answer {
                open_panel: false,
                tail_delta: None,
                full_text: "第一行".into(),
            }
        );
    }

    #[test]
    fn selection_parser_waits_for_visible_text_after_bare_legacy_answer_prefix() {
        let mut with_body = SelectionStreamParser::new();
        let s1 = with_body.push("ANSWER:");
        assert_eq!(s1.decided_route, Some(SelectionRoute::FallbackAnswer));
        assert!(!s1.open_panel);
        assert_eq!(s1.delta, None);

        let s2 = with_body.push("   原因是超时。");
        assert!(s2.open_panel);
        assert_eq!(s2.delta.as_deref(), Some("原因是超时。"));
        assert_eq!(
            with_body.finish(),
            SelectionFinish::Answer {
                open_panel: false,
                tail_delta: None,
                full_text: "原因是超时。".into(),
            }
        );

        let mut bare_only = SelectionStreamParser::new();
        let s1 = bare_only.push("ANSWER:   ");
        assert!(!s1.open_panel);
        assert_eq!(bare_only.finish(), SelectionFinish::Empty);
    }

    #[test]
    fn open_and_close_tag_whitespace_bounds_prevent_unbounded_buffering() {
        let mut parser = SelectionStreamParser::new();
        let huge_open = format!("<rewrite{}", " ".repeat(MAX_OPEN_TAG_LEN));
        let step = parser.push(&huge_open);
        assert_eq!(step.decided_route, Some(SelectionRoute::FallbackAnswer));
        assert!(step.open_panel);

        let within_limit = format!("正文</answer{}", " ".repeat(MAX_CLOSE_TAG_EXTRA_LEN - 1));
        assert!(partial_close_tag_suffix_len(&within_limit, "answer") > 0);
        let over_limit = format!("正文</answer{}", " ".repeat(MAX_CLOSE_TAG_EXTRA_LEN));
        assert_eq!(partial_close_tag_suffix_len(&over_limit, "answer"), 0);

        // max_tokens 截断在半截 `</ans` 闭标签时，finish 剥离残缺协议标签且保留末尾单个 `<`
        let mut truncated = SelectionStreamParser::new();
        let step = truncated.push("<answer>截断前正文</ans");
        assert!(step.open_panel);
        assert_eq!(step.delta.as_deref(), Some("截断前正文"));
        assert_eq!(
            truncated.finish(),
            SelectionFinish::Answer {
                open_panel: false,
                tail_delta: None,
                full_text: "截断前正文".into(),
            }
        );

        let mut trailing_lt = SelectionStreamParser::new();
        trailing_lt.push("<answer>a <");
        assert_eq!(
            trailing_lt.finish(),
            SelectionFinish::Answer {
                open_panel: false,
                tail_delta: Some("<".into()),
                full_text: "a <".into(),
            }
        );
    }
}
