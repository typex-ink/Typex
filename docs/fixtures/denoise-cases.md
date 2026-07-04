# 文本整理测试样例集（F-9）

> 整理提示词的回归基准（08 §6）。每例含输入与**期望要点**（程序化断言依据），非逐字期望输出。
> 执行：`scripts/eval-prompts.ts`（需环境变量密钥）；改动整理提示词的 PR 必须附本地评测报告。

## A. 语气词（中文 ≥10）

| # | 输入 | 期望要点 |
|---|---|---|
| A1 | 嗯……今天的会议改到下午三点 | 不含「嗯」；含「今天的会议改到下午三点」 |
| A2 | 呃，那个，我想请三天假 | 不含「呃」「那个」；含「请三天假」 |
| A3 | 就是说呢，这个方案基本可行 | 不含「就是说呢」；含「方案基本可行」 |
| A4 | 啊对，明天记得带电脑 | 不含「啊对」或仅保留「对」；含「明天记得带电脑」 |
| A5 | 嗯嗯嗯，好的好的，收到 | 结果 ≤ 6 字；含「收到」或「好的」 |
| A6 | 那个……帮我把灯关一下呗 | 不含「那个」；含「关」「灯」 |
| A7 | 唉，这周又要加班了 | 「唉」可留可去；含「这周」「加班」 |
| A8 | 嗯，我觉得吧，还是选第二个方案好 | 不含「我觉得吧」冗余形式（「我觉得」可留）；含「第二个方案」 |
| A9 | 喂喂，测一下麦克风，喂 | 不含重复「喂」；含「测」「麦克风」 |
| A10 | 呃……嗯……周五之前交报告 | 不含语气词；含「周五之前交报告」 |

## B. 语气词（英文 ≥10）

| # | 输入 | 期望要点 |
|---|---|---|
| B1 | um, let's move the meeting to Friday | 不含 "um"；含 "move the meeting to Friday" |
| B2 | so, uh, I think we should refactor this | 不含 "uh"；含 "refactor" |
| B3 | like, this is actually a really good idea | 不含填充词 "like"（句首）；含 "good idea" |
| B4 | well, you know, the deadline is tomorrow | 不含 "you know"；含 "deadline is tomorrow" |
| B5 | hmm okay okay let me check | 不含重复 "okay"；含 "let me check" |
| B6 | I mean, we could just use a cache here | 填充 "I mean" 去除；含 "use a cache" |
| B7 | uh-huh, yes, ship it today please | 含 "ship it today" |
| B8 | er, the API returns, um, a 404 | 不含 "er"/"um"；含 "API"、"404" |
| B9 | basically, um, it's a race condition | 不含 "um"；含 "race condition" |
| B10 | right, so, um, let's start with the tests | 不含 "um"；含 "start with the tests" |

## C. 中途改口（中文/英文 ≥10）

| # | 输入 | 期望要点 |
|---|---|---|
| C1 | 明天下午……不对，是后天下午发布 | 不含「明天」；含「后天下午发布」 |
| C2 | 会议改到八点、呃不对、八点半 | 不含「八点、」孤立表述；含「八点半」 |
| C3 | 把标题改成红色，哦不，蓝色吧 | 不含「红色」；含「蓝色」 |
| C4 | 发给张三……啊说错了，发给李四 | 不含「张三」；含「李四」 |
| C5 | 预算是五万，应该是十五万才对 | 不含孤立「五万」；含「十五万」 |
| C6 | we need three, no wait, four servers | 不含 "three"；含 "four servers" |
| C7 | deploy on Monday — actually, make it Wednesday | 不含 "Monday"；含 "Wednesday" |
| C8 | send it to Alice, sorry I mean Bob | 不含 "Alice"；含 "Bob" |
| C9 | 用 Python 写，唉还是用 Rust 吧 | 不含「Python」；含「Rust」 |
| C10 | 周三交……等等我看下……嗯周五交吧 | 不含「周三」；含「周五」 |

## D. 无意义重复（≥10）

| # | 输入 | 期望要点 |
|---|---|---|
| D1 | 这个这个方案我们再讨论一下 | 「这个」只出现一次 |
| D2 | 我我我觉得没问题 | 「我」开头只一次 |
| D3 | 就是就是说今天先到这里 | 无叠词；含「今天先到这里」 |
| D4 | the the deadline is next week | "the" 不重复 |
| D5 | we should should test it first | "should" 只一次 |
| D6 | 那我们那我们明天再聊 | 无重复片段 |
| D7 | it's a it's a known issue | "it's a" 只一次 |
| D8 | 好好好，就这么定了 | 「好」≤ 2 次或缩为一次；含「就这么定了」 |
| D9 | 然后然后再部署到生产环境 | 「然后」只一次 |
| D10 | run the run the migration script | "run the" 只一次 |

## E. 口述格式指令（≥10）

| # | 输入 | 期望要点 |
|---|---|---|
| E1 | 大家好，另起一段，今天说三件事 | 含换行；不含「另起一段」字样 |
| E2 | 待办第一买菜第二取快递第三交水电费，列成清单 | 含列表符号或编号行；不含「列成清单」字样 |
| E3 | 标题是周报，换行，本周完成了模型接入 | 「周报」后有换行；不含「换行」字样 |
| E4 | first point testing second point deployment, make it a list | 输出为列表；不含 "make it a list" |
| E5 | 优点一速度快二免费三开源，一二三列成清单 | 三项列表 |
| E6 | 亲爱的用户，另起一段，感谢你的反馈 | 两段结构 |
| E7 | new paragraph, let's talk about pricing | 新段落；不含 "new paragraph" |
| E8 | 第一步安装第二步配置第三步运行，分三行 | 三行输出 |
| E9 | 会议纪要冒号，换行，参会人张三李四 | 含「会议纪要：」+ 换行 |
| E10 | bullet points: apples bananas oranges | 三项列表 |

## F. 专有名词保留（≥10）

| # | 输入 | 期望要点 |
|---|---|---|
| F1 | 我们用 Tauri 加 Vue 重写了客户端 | 含「Tauri」「Vue」原样 |
| F2 | 嗯把代码推到 GitHub 上 | 含「GitHub」原样（不改 Github/github） |
| F3 | 那个 Kubernetes 集群又挂了 | 含「Kubernetes」 |
| F4 | 呃我们对接了 DeepSeek 的 API | 含「DeepSeek」「API」 |
| F5 | um, the SQLite database is locked | 含 "SQLite" |
| F6 | Typex 的 HUD 需要重新设计一下 | 含「Typex」「HUD」 |
| F7 | 用 rdev 监听全局按键 | 含「rdev」小写原样 |
| F8 | 这个 PR 里有三个 commit | 含「PR」「commit」 |
| F9 | whisper-large-v3-turbo 的延迟很低 | 模型名原样保留（连字符不丢） |
| F10 | 部署在 Ubuntu 22.04 上 | 含「Ubuntu 22.04」 |

## G. 反向约束：不得过度改写（宁欠勿过，ADR-2）

| # | 输入 | 期望要点 |
|---|---|---|
| G1 | 我今天有点累，想早点休息 | 输出与输入语义一致，不换说法（不得改成「今日疲惫」类） |
| G2 | this is fine | 输出 = "this is fine"（或仅加标点） |
| G3 | 明天见 | 输出 = 「明天见」±标点 |
| G4 | 帮我买杯咖啡，冰的，少糖 | 三个信息点全保留，顺序不变 |
| G5 | 价格是 3999 元不是 4999 元 | 两个数字都保留（这是对比句，非改口） |
