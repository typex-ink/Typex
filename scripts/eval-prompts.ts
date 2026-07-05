// 提示词回归评测（08 §6 / CP-6.10）：读 fixtures 语料 → 真实 API → 要点断言 + 通过率报告。
//
// 用法：
//   TYPEX_EVAL_BASE_URL=https://api.deepseek.com/v1 \
//   TYPEX_EVAL_API_KEY=sk-... \
//   TYPEX_EVAL_MODEL=deepseek-chat \
//   pnpm eval:prompts [denoise|translate|rewrite] [--limit N]
//
// 不进 PR CI（成本与波动）；改动提示词的 PR 必须附本地评测报告（AGENTS.md）。

import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

// ── 内置提示词（与 src-tauri/src/providers/llm/prompt.rs 逐字一致；改模板须同步）──

const POLISH_TEMPLATE = `你是语音转写的后处理引擎。输入是一段语音识别原始文本，输出整理后的文本。
规则：删除语气词与无意义重复；修复标点与断句；
识别说话人的自我修正（如「不对/应该是/我是说」），只保留最终意图；
将口述的格式指令（另起一段、列成清单）转为真实格式；
不增删信息、不改变语言、不替换用词——整理不是改写；
只输出结果本身。
【原始转写】{transcript}`;

const TRANSLATE_TEMPLATE = `你是一个专业翻译引擎。输入是语音转写文本，先在心中还原说话者的真实意图
（忽略语气词、重复与中途改口），再将其从{source_language}翻译为{target_language}。
规则：只输出译文本身；不解释、不加引号、不加任何前后缀；
保留原文的段落、列表与换行结构；语气与正式程度与原文一致；
若原文已经是{bidirectional_target}，则翻译为{bidirectional_source}（双向翻译）。
【原文】{transcript}`;

const PROCESS_TEMPLATE = `你是文本处理引擎。用户选中了一段文本并口述了处理要求。
若要求是对文本的加工（改写/翻译/精简/格式化等）：只输出加工后的文本本身，
不解释、不寒暄，结果将直接替换原文；
若要求实际上是就这段文本提问：以「ANSWER:」开头输出简洁回答。
【选中文本】{selection}
【处理要求】{instruction}`;

// ── LLM 调用 ──

const BASE_URL = process.env.TYPEX_EVAL_BASE_URL?.replace(/\/+$/, "");
const API_KEY = process.env.TYPEX_EVAL_API_KEY;
const MODEL = process.env.TYPEX_EVAL_MODEL;

async function complete(prompt: string): Promise<string> {
  const resp = await fetch(`${BASE_URL}/chat/completions`, {
    method: "POST",
    headers: { "content-type": "application/json", authorization: `Bearer ${API_KEY}` },
    body: JSON.stringify({
      model: MODEL,
      messages: [{ role: "user", content: prompt }],
      temperature: 0.3,
      stream: false,
    }),
  });
  if (!resp.ok) throw new Error(`HTTP ${resp.status}: ${await resp.text()}`);
  const data = (await resp.json()) as { choices: { message: { content: string } }[] };
  return data.choices[0]?.message?.content ?? "";
}

// ── 语料解析：markdown 表 → 用例 ──

interface Case {
  id: string;
  cols: string[]; // 表格各列（去掉 id）
}

function parseTables(file: string): Case[] {
  const text = readFileSync(resolve(ROOT, "docs/fixtures", file), "utf8");
  const cases: Case[] = [];
  for (const line of text.split("\n")) {
    const m = line.match(/^\|\s*([A-Z]\d+)\s*\|(.+)\|\s*$/);
    if (!m) continue;
    const cols = m[2].split("|").map((c) => c.trim());
    cases.push({ id: m[1], cols });
  }
  return cases;
}

// ── 要点断言：解析「含 "x" / 含「x」/ 不含 …」模式 ──

interface Judge {
  pass: boolean;
  detail: string[];
  manual: string[]; // 无法程序化解析的要点
}

function quotedTerms(seg: string): string[] {
  const terms: string[] = [];
  for (const m of seg.matchAll(/[「"']([^」"']+)[」"']|"([^"]+)"/g)) {
    terms.push((m[1] ?? m[2]).trim());
  }
  return terms;
}

function judge(output: string, expectation: string): Judge {
  const lower = output.toLowerCase();
  const detail: string[] = [];
  const manual: string[] = [];
  let pass = true;
  // 以中文分号/顿号切分要点
  for (const seg of expectation.split(/[;；]/)) {
    const s = seg.trim();
    if (!s) continue;
    const terms = quotedTerms(s);
    const isNeg = /^不含|不得|无/.test(s);
    if (!terms.length) {
      // 特殊：长度类「结果 ≤ N 字」
      const lenM = s.match(/[≤<]\s*(\d+)\s*字/);
      if (lenM) {
        const ok = output.length <= Number(lenM[1]);
        if (!ok) pass = false;
        detail.push(`${ok ? "✓" : "✗"} ${s}`);
      } else if (/无中文/.test(s)) {
        const ok = !/[一-鿿]/.test(output);
        if (!ok) pass = false;
        detail.push(`${ok ? "✓" : "✗"} ${s}`);
      } else {
        manual.push(s);
      }
      continue;
    }
    for (const t of terms) {
      const found = lower.includes(t.toLowerCase());
      const ok = isNeg ? !found : found;
      if (!ok) pass = false;
      detail.push(`${ok ? "✓" : "✗"} ${isNeg ? "不含" : "含"}「${t}」`);
    }
  }
  return { pass, detail, manual };
}

// ── 三套评测 ──

async function evalDenoise(limit: number) {
  const cases = parseTables("denoise-cases.md").slice(0, limit);
  return runCases("denoise", cases, async (c) => {
    const [input, expect] = c.cols;
    const out = await complete(POLISH_TEMPLATE.replace("{transcript}", input));
    return { out, expect };
  });
}

async function evalTranslate(limit: number) {
  const cases = parseTables("translate-cases.md").slice(0, limit);
  return runCases("translate", cases, async (c) => {
    const [input, expect] = c.cols;
    const prompt = TRANSLATE_TEMPLATE.replace("{transcript}", input)
      .replaceAll("{source_language}", "中文（简体）")
      .replaceAll("{target_language}", "English")
      .replaceAll("{bidirectional_source}", "中文（简体）")
      .replaceAll("{bidirectional_target}", "English");
    const out = await complete(prompt);
    return { out, expect };
  });
}

async function evalRewrite(limit: number) {
  const cases = parseTables("rewrite-vs-answer-cases.md").slice(0, limit);
  return runCases("rewrite-vs-answer", cases, async (c) => {
    const [selection, instruction, expect] = c.cols;
    const prompt = PROCESS_TEMPLATE.replace("{selection}", selection).replace(
      "{instruction}",
      instruction,
    );
    const out = await complete(prompt);
    // 判定类语料：把「有/无前缀」翻译成可断言要点
    const isAnswer = out.trimStart().startsWith("ANSWER:");
    const wantsAnswer = /有前缀|回答/.test(expect) && !/无前缀/.test(expect);
    const prefixOk = /均可接受/.test(expect) || isAnswer === wantsAnswer;
    return {
      out: `${isAnswer ? "[ANSWER]" : "[REWRITE]"} ${out}`,
      expect,
      overridePass: prefixOk,
    };
  });
}

async function runCases(
  name: string,
  cases: Case[],
  run: (c: Case) => Promise<{ out: string; expect: string; overridePass?: boolean }>,
) {
  let passed = 0;
  const failures: string[] = [];
  for (const c of cases) {
    try {
      const { out, expect, overridePass } = await run(c);
      const j = judge(out, expect);
      const ok = overridePass !== undefined ? overridePass && j.pass : j.pass;
      if (ok) {
        passed += 1;
        process.stdout.write(`  ✓ ${c.id}\n`);
      } else {
        failures.push(`  ✗ ${c.id}\n    输出: ${out.slice(0, 120)}\n    ${j.detail.filter((d) => d.startsWith("✗")).join(" · ")}`);
        process.stdout.write(`  ✗ ${c.id}\n`);
      }
    } catch (e) {
      failures.push(`  ✗ ${c.id} 调用失败: ${e}`);
      process.stdout.write(`  ✗ ${c.id} (error)\n`);
    }
  }
  console.log(`\n[${name}] 通过率 ${passed}/${cases.length} (${((passed / cases.length) * 100).toFixed(0)}%)`);
  if (failures.length) console.log(failures.join("\n"));
  return { passed, total: cases.length };
}

// ── main ──

async function main() {
  if (!BASE_URL || !API_KEY || !MODEL) {
    console.error("缺环境变量：TYPEX_EVAL_BASE_URL / TYPEX_EVAL_API_KEY / TYPEX_EVAL_MODEL");
    process.exit(1);
  }
  const args = process.argv.slice(2);
  const which = args.find((a) => !a.startsWith("--")) ?? "all";
  const limitIdx = args.indexOf("--limit");
  const limit = limitIdx >= 0 ? Number(args[limitIdx + 1]) : Infinity;

  const results: { passed: number; total: number }[] = [];
  if (which === "all" || which === "denoise") results.push(await evalDenoise(limit));
  if (which === "all" || which === "translate") results.push(await evalTranslate(limit));
  if (which === "all" || which === "rewrite") results.push(await evalRewrite(limit));

  const passed = results.reduce((a, r) => a + r.passed, 0);
  const total = results.reduce((a, r) => a + r.total, 0);
  console.log(`\n总计 ${passed}/${total} (${((passed / total) * 100).toFixed(0)}%)`);
  process.exit(passed === total ? 0 : 2);
}

main();
