# 翻译测试样例集（F-2）

> 翻译提示词的回归基准（07 §6）。对应 F-2 验收标准 1/3：双向各 20 句、含列表结构保留。
> 每例含输入与**期望要点**（程序化断言依据），非逐字期望输出。
> 自动评测器必须复用运行时提示词；不要在脚本中维护单独提示词副本。
> 语言对固定为「中文（简体） ↔ English」（默认设置）；双向开关开启。

## A. 中 → EN（≥20）

判定要点通用规则：结果不得包含中文字符（即「无一句注入原文未翻译」）；下列「含」均为大小写不敏感的英文关键词。

| # | 输入 | 期望要点 |
|---|---|---|
| A1 | 今天的会议改到下午三点 | 含 "meeting"、"3"（或 "three"）；无中文 |
| A2 | 帮我看一下这个 bug 是什么原因 | 含 "bug"；无中文 |
| A3 | 我想请三天假 | 含 "three days"（或 "3 days"）与 "leave"；无中文 |
| A4 | 这个方案基本可行，下周开始执行 | 含 "next week"；无中文 |
| A5 | 麻烦把报告在周五之前发给我 | 含 "Friday"、"report"；无中文 |
| A6 | 嗯……那个，预算大概是十五万 | 不含 "um" 类填充词；含 "150"；无中文 |
| A7 | 明天下午……不对，是后天下午发布 | 不含 "tomorrow"（改口只留最终意图）；含 "release" 或 "launch"；无中文 |
| A8 | 服务器需要四台，内存至少 32G | 含 "four"（或 "4"）、"32"；无中文 |
| A9 | 这段代码有内存泄漏的风险 | 含 "memory leak"；无中文 |
| A10 | 你晚饭想吃什么？ | 以问句形式结尾（含 "?"）；含 "dinner"；无中文 |
| A11 | 第一，先跑测试；第二，再部署；第三，观察日志 | 保留三条列表/序号结构（"first/second/third" 或 1/2/3）；无中文 |
| A12 | 会议纪要：一、确定排期。二、分配任务。 | 保留两条结构；含 "schedule"（或 "timeline"）；无中文 |
| A13 | 把标题改成蓝色，字号大一点 | 含 "blue"、"title"；无中文 |
| A14 | 项目延期两周，原因是依赖的接口还没就绪 | 含 "two weeks"、"API"（或 "interface"/"dependency"）；无中文 |
| A15 | 辛苦了，今天先到这里，明天继续 | 含 "tomorrow"；无中文 |
| A16 | 这个需求优先级不高，放到下个迭代 | 含 "priority"、"next"；无中文 |
| A17 | 记得备份数据库再执行迁移脚本 | 含 "backup"、"migration"（或 "migrate"）；无中文 |
| A18 | 快递明天上午送到，记得有人签收 | 含 "tomorrow morning"（或 "tomorrow"）；无中文 |
| A19 | 用户反馈登录页面偶尔白屏 | 含 "login"；无中文 |
| A20 | 价格含税一共是三千二百块 | 含 "3,200"（或 "3200"）、"tax"；无中文 |
| A21 | 我们用瑞艾克特重构了这个 APP | 含 "React"、"App"、"refactor"（或 "rewrote"）；无中文 |

## B. EN → 中（双向自动判向，≥20）

判定要点通用规则：结果必须以中文为主体（含中文字符；技术专名/产品名可保留英文）；即说英文自动译回中文，不得原样返回英文。

| # | 输入 | 期望要点 |
|---|---|---|
| B1 | let's move the meeting to Friday | 含「周五」与「会议」 |
| B2 | the deadline is next Wednesday | 含「周三」（或「下周三」）与「截止」（或「期限」） |
| B3 | um, I think we should refactor this module | 不含填充词；含「重构」「模块」 |
| B4 | can you send me the report by tomorrow? | 问句语气；含「明天」「报告」 |
| B5 | the API returns a 404 on the login page | 含「404」「登录」；"API" 可保留英文 |
| B6 | we need four servers with at least 32 gigs of RAM | 含「四台」（或「4 台」）「32」「内存」 |
| B7 | first, run the tests; second, deploy; third, watch the logs | 保留三条结构；含「测试」「部署」「日志」 |
| B8 | this is a race condition, not a memory leak | 含「竞态」（或「竞争条件」）「内存泄漏」；语义为「是…不是…」 |
| B9 | ship it today please | 含「今天」；语义为发布/上线 |
| B10 | the budget is one hundred fifty thousand | 含「十五万」（或「15 万」/「150,000」） |
| B11 | send it to Alice, sorry I mean Bob | 不含「Alice」；含「Bob」 |
| B12 | I'll be five minutes late to the standup | 含「五分钟」（或「5 分钟」）「迟到」 |
| B13 | what do you want for dinner? | 问句；含「晚饭」（或「晚餐」） |
| B14 | the new feature is behind a feature flag | 含「开关」（或「feature flag」保留）；语义为「在…之后/受控」 |
| B15 | don't forget to back up the database before the migration | 含「备份」「数据库」「迁移」 |
| B16 | this bug only reproduces on macOS | 含「macOS」「复现」（或「重现」） |
| B17 | let's postpone the launch by two weeks | 含「两周」「推迟」（或「延期」） |
| B18 | the package will arrive tomorrow morning | 含「明天上午」（或「明早」） |
| B19 | users report a blank screen on the login page sometimes | 含「白屏」（或「空白」）「登录」 |
| B20 | the total is three thousand two hundred including tax | 含「三千二」（或「3200」）「含税」 |
| B21 | fix the my sequel connection in VS code | 含「MySQL」「VS Code」「连接」 |

## C. 结构保留（验收标准 3 专项）

| # | 输入 | 期望要点 |
|---|---|---|
| C1 | 待办事项：第一，回复邮件。第二，写周报。第三，订会议室。 | 译文保留三条清单结构（序号或列表符号），顺序不变 |
| C2 | 两点说明：一是价格不变，二是交期提前 | 译文保留两点结构 |
| C3 | there are three steps: clone the repo, install dependencies, and run the dev server | 中文译文保留三步结构（顿号/序号/换行均可） |
| C4 | 注意换行：（换行）这是第二段 | 译文保留段落换行 |
