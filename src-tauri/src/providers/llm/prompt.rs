//! PromptKit：内置提示词模板 + 占位符渲染（03 §3.4）。
//!
//! 规则：必需占位符缺失 = 校验失败；含可选占位符的**行**在值不存在时整行省略。

use std::collections::HashMap;

/// 内置模板：文本整理（F-9，「文本整理」槽）——03 §3.4 逐字。
pub const POLISH_TEMPLATE: &str = "\
你是语音转写的后处理引擎。输入是一段语音识别原始文本，输出整理后的文本。
规则：删除语气词与无意义重复；修复标点与断句；
识别说话人的自我修正（如「不对/应该是/我是说」），只保留最终意图；
将口述的格式指令（另起一段、列成清单）转为真实格式；
不增删信息、不改变语言、不替换用词——整理不是改写；
只输出结果本身。
以下专有名词按原样保留：{dictionary}
【原始转写】{transcript}";

/// 内置模板：翻译（F-2，「翻译模型」槽）。
pub const TRANSLATE_TEMPLATE: &str = "\
你是一个专业翻译引擎。输入是语音转写文本，先在心中还原说话者的真实意图
（忽略语气词、重复与中途改口），再将其从{source_language}翻译为{target_language}。
规则：只输出译文本身；不解释、不加引号、不加任何前后缀；
保留原文的段落、列表与换行结构；语气与正式程度与原文一致；
若原文已经是{bidirectional_target}，则翻译为{bidirectional_source}（双向翻译）。
【原文】{transcript}";

/// 内置模板：文本处理（F-3a，「问答模型」槽）。
pub const PROCESS_TEMPLATE: &str = "\
你是文本处理引擎。用户选中了一段文本并口述了处理要求。
若要求是对文本的加工（改写/翻译/精简/格式化等）：只输出加工后的文本本身，
不解释、不寒暄，结果将直接替换原文；
若要求实际上是就这段文本提问：以「ANSWER:」开头输出简洁回答。
【选中文本】{selection}
【处理要求】{instruction}";

/// 内置模板：语音问答（F-3b，「问答模型」槽）。
pub const ASK_TEMPLATE: &str = "\
你是 Typex 语音助手。用户通过语音提出一个问题，这是单轮问答。
回答应直接、简洁、可立即使用；默认使用用户提问的语言。
用户当前选中的内容作为上下文：{selection}
【问题】{instruction}";

/// F-3a「改写 vs 回答」判定信号（03 §3.4）。
pub const ANSWER_PREFIX: &str = "ANSWER:";

/// 各槽位的必需占位符（保存校验用，05 §5.2）。
pub fn required_placeholders(slot: crate::types::SlotKind) -> &'static [&'static str] {
    use crate::types::SlotKind;
    match slot {
        SlotKind::Polish => &["{transcript}"],
        SlotKind::Translate => &["{transcript}", "{source_language}", "{target_language}"],
        SlotKind::Assistant => &["{instruction}"],
        SlotKind::Stt => &[],
    }
}

/// 校验自定义模板：必需占位符必须全部出现。
pub fn validate(template: &str, slot: crate::types::SlotKind) -> Result<(), Vec<String>> {
    let missing: Vec<String> = required_placeholders(slot)
        .iter()
        .filter(|p| !template.contains(*p))
        .map(|p| p.to_string())
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing)
    }
}

/// 渲染模板：
/// - `values` 中存在的占位符 → 替换；
/// - 行内含**不在 values 中**的占位符 → 整行省略（可选段规则，03 §3.4）。
pub fn render(template: &str, values: &HashMap<&str, String>) -> String {
    let mut out_lines = Vec::new();
    'line: for line in template.lines() {
        let mut rendered = line.to_string();
        // 找出本行所有 {placeholder}
        let mut rest = line;
        while let Some(start) = rest.find('{') {
            let Some(end_rel) = rest[start..].find('}') else {
                break;
            };
            let ph = &rest[start..start + end_rel + 1];
            match values.get(ph) {
                Some(v) => rendered = rendered.replace(ph, v),
                None => continue 'line, // 值不存在 → 整行省略
            }
            rest = &rest[start + end_rel + 1..];
        }
        out_lines.push(rendered);
    }
    out_lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SlotKind;

    #[test]
    fn render_replaces_placeholders() {
        let mut v = HashMap::new();
        v.insert("{transcript}", "你好".to_string());
        v.insert("{dictionary}", "Typex".to_string());
        let out = render(POLISH_TEMPLATE, &v);
        assert!(out.contains("【原始转写】你好"));
        assert!(out.contains("以下专有名词按原样保留：Typex"));
    }

    #[test]
    fn optional_line_omitted_when_value_missing() {
        let mut v = HashMap::new();
        v.insert("{transcript}", "你好".to_string());
        // 不提供 {dictionary} → 该行整体省略
        let out = render(POLISH_TEMPLATE, &v);
        assert!(!out.contains("专有名词"));
        assert!(out.contains("【原始转写】你好"));
    }

    #[test]
    fn validate_detects_missing_required() {
        assert!(validate("没有占位符的模板", SlotKind::Polish).is_err());
        assert!(validate("好模板 {transcript}", SlotKind::Polish).is_ok());
        let err = validate("{transcript} only", SlotKind::Translate).unwrap_err();
        assert!(err.contains(&"{source_language}".to_string()));
        assert!(err.contains(&"{target_language}".to_string()));
    }

    #[test]
    fn translate_template_renders_both_languages() {
        let mut v = HashMap::new();
        v.insert("{transcript}", "这个 bug 我明天修".to_string());
        v.insert("{source_language}", "中文".to_string());
        v.insert("{target_language}", "English".to_string());
        v.insert("{bidirectional_source}", "中文".to_string());
        v.insert("{bidirectional_target}", "English".to_string());
        let out = render(TRANSLATE_TEMPLATE, &v);
        assert!(out.contains("从中文翻译为English"));
        assert!(out.contains("若原文已经是English，则翻译为中文"));
    }

    #[test]
    fn translate_template_omits_bidirectional_line_when_disabled() {
        let mut v = HashMap::new();
        v.insert("{transcript}", "hello".to_string());
        v.insert("{source_language}", "中文".to_string());
        v.insert("{target_language}", "English".to_string());
        // 不注入 {bidirectional_*} = 双向翻译关闭
        let out = render(TRANSLATE_TEMPLATE, &v);
        assert!(!out.contains("双向翻译"));
        assert!(out.contains("从中文翻译为English"));
        assert!(out.contains("【原文】hello"));
    }

    #[test]
    fn ask_template_selection_optional() {
        let mut v = HashMap::new();
        v.insert("{instruction}", "现在几点".to_string());
        let out = render(ASK_TEMPLATE, &v);
        assert!(!out.contains("选中的内容"));
        assert!(out.contains("【问题】现在几点"));
    }

    #[test]
    fn answer_prefix_detection() {
        assert!("ANSWER: 这是回答".starts_with(ANSWER_PREFIX));
        assert!(!"这是改写结果".starts_with(ANSWER_PREFIX));
    }
}
