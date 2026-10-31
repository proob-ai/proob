<!-- translated from README.md sha256:c8d03c3cc3cab1a0262cb7223eb247a69e7ec1c864cb376f98e1f436617712a7 -->
# proob

[![ci](https://github.com/proob-ai/proob/actions/workflows/ci.yml/badge.svg)](https://github.com/proob-ai/proob/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

[English](README.md) · **简体中文** · [日本語](README.ja.md)

**用平实语言写下的代码保证：由 AI 智能体提出，由你的团队批准，由机器在每个拉取请求上检查。**

每个代码库都有一些没人写下来的规则。退款永远不超过扣款。版本字符串解析回来仍是同一个版本。下一个发布号总是比上一个大。测试只覆盖有人记得的情况。proob 把这些规则写成*性质*，每条一句话，并为每条配一个检查，用成千上万个生成的输入去寻找能打破它的那个用例。从此以后，打破规则的那个拉取请求就是会失败的那个。

![在 Claude Code 中对 python-semver 运行 /proob:try：五条性质，两条失败，其中一条是真实缺陷](docs/try.gif)

在 Claude Code 中对 python-semver 的一次真实运行，代码回退到维护者修复 [issue #497](https://github.com/python-semver/python-semver/issues/497) 之前的那个提交。一条命令，五分钟。proob 提出五条性质，为每条编写并运行了检查，并找到了维护者后来修复的缺陷：请求*下一个*预发布版本可能返回一个*更旧*的版本。它还找到了第二个，在版本哈希上。仓库里没有任何东西被改动。

## 为什么选择 proob

- **一条性质覆盖所有输入；一个测试只覆盖一个。** “打印一个版本再解析回来得到同一个版本”会在成千上万个生成的版本上检查，而不是某人敲下的那三个。
- **由你的团队决定什么算数。** 智能体只负责提议。一条性质只有在队友通过普通的拉取请求审查批准后才生效，而 proob 每次都从 GitHub 读取这个批准，所以无论智能体还是单人合并都无法伪造它。
- **绿色是有含义的。** proob 会变异你的代码，以确认每个检查确实可能失败。无论如何都通过的检查会被报告为弱，而不是证明。

## 它真的能找到缺陷吗？

我们从流行的 Python 和 Rust 库中收集了 15 个缺陷，包括 more-itertools、cattrs、packaging、tomlkit、bytes 和 time。每一个都曾逃过该库自己的测试套件，后来由维护者修复。我们把每个库回退到修复之前的提交，然后在没有任何提示的情况下运行 proob。

**它找到了 15 个中的 14 个。** 其中 10 个缺陷是在 AI 模型的训练数据截止之后才修复的，所以它不可能记得它们。这 10 个中它找到了 9 个。

在 attrs、rich、vaultwarden、python-semver、rust-url 和 tomlkit 的当前版本上运行时，它找到了 **12 个仍然存在于代码中的缺陷**。它报告的失败中，大约每七个有六个是真实缺陷（59 个中的 50 个）。其余的是对库作出了超出其承诺的假设的检查。

在 501 个模拟场景中（其中一些专门用来欺骗它），proob **从未把一条已被破坏的性质报告为成立**。

它找到的每个缺陷都会成为回归测试，由 CI 在没有 AI 参与的情况下重放，所以 proob 不会悄悄变差。我们如何测量，以及每一个数字：[docs/benchmark.md](docs/benchmark.md)。

## 快速开始

在 Claude Code 中，于你的仓库内：

```text
/plugin marketplace add proob-ai/proob
/plugin install proob@proob
/proob:try
```

`/proob:try` 在 `~/.proob/shadow/` 下的私有副本中工作，所以你的文件、`.git` 和 `git status` 都保持原样。你需要带有 pytest、Hypothesis 和 mutmut 的 Python，或带有 cargo-nextest 和 cargo-mutants 的 Rust（[详情](docs/guide.md#install)）。

如果你认可它的发现，报告末尾有保留它们的三个步骤：

1. `proob shadow adopt` 把性质、它们的检查和两个工作流放到一个新分支上。
2. 打开拉取请求。队友的审查即是批准。
3. 在默认分支上把 `proob gate` 检查设为必需。

## 工作原理

proob 在你已有的流程上只增加一个必需检查和一个审查习惯。此后，团队约定的每条规则都会在每次变更时重新检查，打破规则的变更无法合并，而没有规则覆盖的新代码会被指出，但绝不会被阻塞。

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/flow-dark.svg">
  <img alt="一次性：/proob:try，然后 proob shadow adopt，然后把 proob gate 设为必需检查。此后的每个拉取请求：gate 重新检查该变更触及的性质；全部成立则合并并在夜间刷新；有一条被破坏则阻止合并；没有性质覆盖的新代码会在同一次审查中得到提议。" src="docs/flow-light.svg" width="960">
</picture>

| 技能 | 作用 |
|---|---|
| `/proob:sense` | 智能体阅读你的代码 |
| `/proob:declare` | 把性质提议为平实语言的文件，放进由团队审查的拉取请求 |
| `/proob:falsify` | 把每条已批准的性质绑定到一个基于性质的测试，或在 Rust 中绑定到一个 [Verus](https://github.com/verus-lang/verus) 证明 |
| `/proob:verify` | proob 变异代码，表明每个检查确实可能失败 |
| `/proob:guard` | 每个拉取请求、每晚、每次会话：哪些性质仍然成立 |

每条性质都有一个状态：`Holds`、`Checked`、`Not checked`、`Re-approve`、`Broken`、`Proposed` 或 `Waived`。`Holds` 表示由作者以外的审查者批准、有来自默认分支的新鲜证据，并且检查被证明能够失败。`Holds` 只保留给证明或模型，基于性质的测试最高达到 `Checked (tested)`。只做审查的队友只需要 GitHub。[指南](docs/guide.md)（英文）介绍每个技能、各个状态、团队设置和命令行。

## 智能体能作弊吗？

不能。

- **批准从不存储在仓库中。** 每次显示状态时都从 GitHub 的审查记录中读取，没有第二位审查者就合并的变更永远不算数。
- **证据只来自你的默认分支，** 由一个不持有令牌的作业产生。
- **把批准和证据转化为结论的代码是经机器证明正确的，** 用 Verus 证明，而不只是测试。它所对照证明的[六条断言](crates/proob-core/ASSERTIONS.md)短到一杯咖啡的时间就能读完。

## 路线图

**MVP。** 一个把基于性质的测试和形式化验证视为同一种检查的两个类别的核心，以及围绕它的一个循环：

- [x] **为你的代码库共同撰写第一份声明。** 智能体阅读代码并提出规则；团队在拉取请求中批准
- **从声明生成检查**，让它们抓住你的测试遗漏的缺陷
  - [x] 作为基于性质的测试，支持 Python 和 Rust
  - [x] 作为证明，针对用 Verus 编写的 Rust
  - [x] 作为普通 Rust 上的证明，无需编写证明（Kani）
  - [ ] 作为普通 Python 上的证明，无需编写证明（CrossHair）
  - [ ] 作为你的设计的形式化规约，由智能体根据你的代码库选择合适的语言（协议用 TLA+，数据模型用 Alloy）
  - [ ] 验证优先，并有安全着陆：当没有验证器能处理某条规则时（不支持的特性、无界循环、超时），智能体改为绑定一个基于性质的测试，并如实说明
  - [x] 结论区分测试过与证明过：`Holds` 意味着已证明，测试显示为 `Checked (tested)`
- [x] **守护每个拉取请求。** 打破已批准规则的变更无法合并
- **守护规则与代码之间的漂移。** 规则绝不会悄悄地对它不再描述的代码保持已批准
  - [x] 规则或其检查发生变化，必须重新批准
  - [x] 没有规则覆盖的新代码会在拉取请求中被指出
  - [ ] 偏离其设计规约的代码会把规则标记为待更新并重新批准

**之后**

- [ ] 在 TypeScript 中使用，然后是 Go 和 Java
- [ ] 向审计者交付一份可以离线独立成立的结论（签名证据）
- [ ] 同时守护所有仓库，批准仍然从 GitHub 读取

### 支持的语言

**测试负责找缺陷；只有验证器能赢得 `Holds`。** 每条性质绑定一个检查。基于性质的测试在生成的输入中搜索反例：找到则为 `Broken`，没找到则为 `Checked (tested)`，因为抽样运行不能代表所有输入。证明或模型检查器覆盖所有输入，所以只有它能给出 `Holds`。Kani 证明覆盖其 harness 所设界限内的所有输入，结论会如实说明。结论始终说明背后是哪一类证据。接受得过于轻易的验证器会像弱测试一样被抓住：proob 在证明之下变异代码，仍然通过的证明会被报告为空洞，而不是成立。

☑ 已构建，☐ 计划中。

| 语言 | 形式化验证 | 基于性质的测试 |
|---|---|---|
| Rust | ☑ Verus，针对 Verus 代码 · ☑ Kani，针对任何 Rust | ☑ proptest |
| Python | ☐ CrossHair | ☑ Hypothesis |
| 设计层面，任何语言 | ☐ TLA+ 与 TLC、Alloy | |
| TypeScript | | ☐ fast-check |
| Go、Java | | ☐ |

在其他任何语言中，sense 和 declare 仍可使用，每条性质显示为 `Not checked (no adapter)`。已知局限见[指南](docs/guide.md#limits)。

## 参与贡献

问题请发到 [Discussions](https://github.com/proob-ai/proob/discussions)，缺陷请提交到 [issues](https://github.com/proob-ai/proob/issues)，安全漏洞请通过私密安全公告报告（[`SECURITY.md`](SECURITY.md)）。欢迎向 `master` 提交拉取请求；[`CONTRIBUTING.md`](CONTRIBUTING.md) 说明了各项检查，`cargo xtask ci` 会全部运行。编辑英文 README 后，请更新每个 `README.<language>.md` 及其源哈希；`cargo xtask readme-sync` 会确认它们一致。

## 许可证

[Apache-2.0](LICENSE) © proob-ai
