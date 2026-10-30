<!-- translated from README.md sha256:36d235ecabaff63dbe5d07af9995b5ba711856af103ba0aa1d206c707ca9be24 -->
# proob

[![ci](https://github.com/proob-ai/proob/actions/workflows/ci.yml/badge.svg)](https://github.com/proob-ai/proob/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

[English](README.md) · **简体中文** · [日本語](README.ja.md)

**你的测试只检查有人想到的情况。proob 找出没人想到要测的缺陷。**

proob 是一个 Claude Code 插件。它阅读你的代码，用平实的语言写下必须始终成立的规则（例如“退款永远不超过原始扣款”），再用成千上万个自动生成的输入去攻击每条规则。团队在一次普通的拉取请求审查中批准这些规则，此后 proob 在每个拉取请求上重新检查它们。智能体只负责提议，批准只能来自你的团队。

![在 Claude Code 中对 python-semver 运行 /proob:try：五条性质，其中一条因真实缺陷而失败](docs/try.gif)

在 Claude Code 中对 python-semver 真实运行 `/proob:try`，时间点是维护者修复 issue #497 之前的一个提交，中间约五分钟的等待已剪去。proob 提出五条性质，其中一条失败：请求下一个预发布版本时可能倒退（`1.2.3-rc.1` 使用标记 `alpha` 得到 `1.2.3-alpha.1`，而它排在前者之前）。这正是维护者后来修复的缺陷。仓库里没有任何文件被改动。

## 它真的有效吗？

每个版本都用逃过真实测试套件的真实缺陷来衡量，方法借自测试研究（Defects4J、Klees 等人 CCS 2018、SV-COMP），而非我们自创。

| 测试 | 结果 |
|---|---|
| 在模型训练截止日期**之后**才修复的真实缺陷，模型不可能记住它们 | **10 个中发现 9 个** |
| 留出的历史缺陷，盲测 | **20 次试验中 18 次**（90%） |
| 在 attrs、rich、vaultwarden、python-semver、rust-url、tomlkit 当时最新版本中发现的未知缺陷 | **12 个** |
| 在 260 个计分结论中，代码违反规则而 proob 却判定 `Holds` 的次数 | **0** |
| 报告的失败中确为真实缺陷的比例 | 59 个中 50 个（85%） |

每次发现都会成为回归测试，由 CI 在没有模型参与的情况下重放；绑定的检查能在十秒内再次找到该缺陷。完整数据：[docs/benchmark.md](docs/benchmark.md)。

## 快速开始

在 Claude Code 中，于你的仓库内：

```text
/plugin marketplace add proob-ai/proob
/plugin install proob@proob
/proob:try
```

`/proob:try` 在 `~/.proob/shadow/` 下的私有副本中工作，你的文件、`.git` 和 `git status` 都保持原样。你需要带有 pytest、Hypothesis 和 mutmut 的 Python，或带有 cargo-nextest 和 cargo-mutants 的 Rust（[详情](docs/guide.md#install)）。

如果你认可它的发现，报告末尾给出保留它们的三个步骤：

1. `proob shadow adopt` 把规则、测试和两个工作流放到一个新分支上。
2. 打开拉取请求。队友的审查即是批准。
3. 在默认分支上把 `proob gate` 检查设为必需。

## 工作原理

| 技能 | 作用 |
|---|---|
| `/proob:sense` | 智能体阅读你的代码 |
| `/proob:declare` | 把规则提议为平实语言的文件，放进由团队审查的拉取请求 |
| `/proob:falsify` | 为每条已批准的规则编写基于性质的测试，用固定种子运行 |
| `/proob:verify` | proob 变异代码，证明每个测试确实可能失败 |
| `/proob:guard` | 每个拉取请求、每晚、每次会话：哪些规则仍然成立 |

每条规则都有一个状态：`Holds`、`Checked`、`Not checked`、`Re-approve`、`Broken`、`Proposed` 或 `Waived`。`Holds` 表示由作者以外的审查者批准、有来自默认分支的新鲜证据，并且检查它的测试被证明能够失败。只做审查的队友只需要 GitHub。[指南](docs/guide.md)（英文）介绍每个技能、各个状态、团队设置和命令行。

## 为什么可以信任结论

- **智能体无法批准任何东西。** 每次显示状态时，批准都从 GitHub 的审查记录中读取；仓库里不存储批准，自己合并的变更永远不算。
- **证据只来自默认分支，** 由一个不持有令牌的作业产生。
- **做出判断的代码经过形式化验证。** 结论函数用 [Verus](https://github.com/verus-lang/verus) 对照定义 `Holds` 的[六条断言](crates/proob-core/ASSERTIONS.md)证明；可信基只有[两个函数](crates/proob-trusted/README.md)。

## 支持的语言

| 语言 | 检查 | 强度 |
|---|---|---|
| Python | 通过 pytest 运行 Hypothesis | mutmut |
| Rust | 通过 cargo-nextest 运行 proptest | cargo-mutants |
| Verus 下的 Rust | 验证器 | 证明下的变异 |
| TypeScript | 计划中：通过 vitest 运行 fast-check | StrykerJS |

在其他语言中，sense 和 declare 仍可使用，每条规则显示 `Not checked (no adapter)`。已知局限见[指南](docs/guide.md#limits)。

## 参与贡献

问题请发到 [Discussions](https://github.com/proob-ai/proob/discussions)，缺陷请提交到 [issues](https://github.com/proob-ai/proob/issues)，安全漏洞请通过私密安全公告报告（[`SECURITY.md`](SECURITY.md)）。欢迎向 `master` 提交拉取请求；[`CONTRIBUTING.md`](CONTRIBUTING.md) 说明了各项检查，`cargo xtask ci` 会全部运行。编辑英文 README 后，请更新每个 `README.<language>.md` 及其源哈希；`cargo xtask readme-sync` 会确认它们一致。

## 许可证

[Apache-2.0](LICENSE) © proob-ai
