<!-- translated from README.md sha256:23d7eaf9d61ad234f052038c63b4380d24366f8019f00d93c28035059ccd7c86 -->
# proob

[![ci](https://github.com/proob-ai/proob/actions/workflows/ci.yml/badge.svg)](https://github.com/proob-ai/proob/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

[English](README.md) · **简体中文** · [日本語](README.ja.md)

由团队批准、由机器检查、持续守护的代码性质。

你的团队用平实的语言约定代码绝不能做什么；proob 在每个拉取请求上检查这些约定，并在它们不再成立的那一天告诉你。智能体阅读代码并提出这些陈述，团队通过审查拉取请求来批准，而每一个结论都来自在默认分支上实际运行过的测试和证明。

**在 11 个逃过了知名开源库自身测试套件的真实缺陷上，proob 的智能体在不知情的情况下发现了其中 10 个，33 次运行中有 22 次命中；另外 10 个在其模型训练截止日期之后才修复的缺陷中，它发现了 9 个。** 它写出的检查能在十秒内再次找到每个缺陷。在 attrs、rich 和 vaultwarden 上，它发现了 7 个在最新发布版本中仍然存在的真实缺陷。详见[基准测试](#基准测试)。想先看看它在你自己的代码里能发现什么、又不改动仓库，请运行 [`/proob:try`](#不改动仓库先试一试)。

```text
$ claude   # in a checkout of python-semver, one commit before its fix for issue #497
> /proob:try

shadow: 5 properties tried · 1 fails · 4 pass · nothing in your repository was changed

  - Asking for the next version always gives a strictly greater version
    fails: v=parse('0.0.0-1'), part='prerelease', token='0'
    0.0.0-1.next_version('prerelease', '0') gave 0.0.0-0.1
    replay: proob --shadow run check 01M644W0MSK9CBE2136XR777N5 --seed 1793003282928823768
  - Printing a parsed version string gives back exactly that string · passes, test can fail
  - Two versions that compare equal always have the same hash · passes, test can fail
  - Version ordering is antisymmetric and transitive · passes, test can fail
  - Bumping a prerelease or build always yields valid SemVer · passes, test can fail

Make it real:
  0. fix the code or drop the failing property before adopting
  1. proob shadow adopt   puts the properties, tests, and workflows on a new branch
  2. open a pull request; a teammate's review is the approval
  3. make the proob gate check required on main
```

一次真实的 `/proob:try` 运行（有删节）：一条命令，约五分钟，1.10 美元。proob 找到了 python-semver 维护者后来修复的那个缺陷（求下一个预发布版本时可能倒退，例如 `1.2.3-rc.1` 加上 token `alpha` 得到 `1.2.3-alpha.1`），并且没有改动仓库中的任何文件。

## 目录

- [背景](#背景)
- [安装](#安装)
- [使用](#使用)
- [基准测试](#基准测试)
- [我们如何确认它可靠](#我们如何确认它可靠)
- [支持的语言](#支持的语言)
- [团队使用](#团队使用)
- [局限](#局限)
- [参与贡献](#参与贡献)
- [许可证](#许可证)

## 背景

测试只检查有人想到要测的东西。形式化方法能检查必须始终成立的东西，但需要专家，而且容易与代码脱节。proob 保留真正重要的部分，也就是团队约定的一份简短性质清单，其余一切都交给机器完成。

| 阶段 | 由谁执行 | 发生什么 |
|---|---|---|
| Sense（感知） | 智能体 | 阅读代码，把它对系统的理解写入本地缓存 |
| Declare（声明） | 智能体，然后是人 | 以文件形式提出用平实语言写成的性质；团队在拉取请求中批准 |
| Falsify（证伪） | 智能体，然后是机器 | 为每条性质写一个基于性质的测试；proob 以固定的种子和预算运行它 |
| Verify（验证） | 机器 | 通过变异被覆盖的代码，衡量每个检查是否真的能失败 |
| Guard（守护） | 机器 | 在每个拉取请求、每天夜里以及每次会话开始时重新推导每个结论 |

智能体只提议，从不批准。每次显示状态时，批准都从 GitHub 的审查记录中实时读取；仓库里不保存任何批准。

proob 对自身的检查结果，根据本仓库的审查记录和默认分支上的运行实时生成：

| Property | Status | Reason | This PR |
|---|---|---|---|
| A statement nobody else approved never holds `01M622RQ71P6ZXCKVVTRCA4DSE` | Re-approve | _check amended_ | not run |
| An unapproved check, config, or workflow never holds `01M622RQ72HFMBG9RTQBMB7YBE` | Re-approve | _check amended_ | not run |
| Stale evidence never holds `01M622RQ73XGJ3PRRVCY53FECA` | Re-approve | _check amended_ | not run |
| A self-signed merge never holds `01M622RQ74SFCQ8Y800YE3XNS9` | Re-approve | _check amended_ | not run |
| A waiver lapses with its issue or its window `01M622RQ75T3CFHXPNAAHR3PPD` | Re-approve | _check amended_ | not run |
| Approval comes only from an eligible review `01M622RQ76XZBSVRNHW3PC3Z5S` | Re-approve | _check amended_ | not run |
| The scope hash ignores file order `01M622RQ778BXGXXT6ZQ54EJFR` | Re-approve | _check amended_ | not run |
| The gate blocks only a broken property this change affects `01M622RQ781KT06FB0MKKCZNPF` | Re-approve | _check amended_ | not run |
| Evidence comes only from the approved workflow on the default branch `01M622RQ791MXW80SMN13TWVSY` | Re-approve | _check amended_ | not run |
| Property files survive a round trip `01M622RQ7AM90T2YNQHJ6P58PW` | Checked (strength pending, self-signed) | _strength not yet measured; no second reviewer_ | not run |
| The fact cache cannot write a verdict `01M622RQ7BGJ75BQ4B0JARP2KC` | Checked (strength pending, self-signed) | _strength not yet measured; no second reviewer_ | not run |
| A run off the default branch is not evidence `01M622RQ7C066YFA9NDMB7EG6K` | Checked (strength pending, self-signed) | _strength not yet measured; no second reviewer_ | not run |
| Property-derived arguments follow the separator `01M622RQ7D1NN8MNXJA62ABZNB` | Checked (strength pending, self-signed) | _strength not yet measured; no second reviewer_ | not run |

四条由测试支撑的性质显示 `Checked (self-signed)`：已在默认分支上检查过，因为本仓库只有一位维护者，所以批准时没有第二位审查者，并且还在等待第一次夜间强度评分。九条由证明支撑的性质显示 `Re-approve`，因为有一次提交没有经过审查过的拉取请求，就修改了这些证明所依赖的文件。这正是规则在起作用：无人审查的改动不能保留原有的批准。`Holds` 需要第二位维护者，在那之前这里不会有任何一行显示它。

## 安装

在 Claude Code 中：

```text
/plugin marketplace add proob-ai/proob
/plugin install proob@proob
```

或者在 Claude Code 2.1.275 及以上版本中一步完成：

```text
/plugin install proob --marketplace proob-ai/proob
```

第一次运行某个技能时，插件会从本仓库的 GitHub Releases 下载适合你平台的 `proob` 二进制文件。它会用发布的 `.sha256` 文件核对 SHA-256，在发布附带证明且 `gh` 已登录时核对 Sigstore 证明（本仓库仍为私有期间的发布不附带），并把二进制缓存到 `~/.proob/bin/`。校验和不符的文件绝不会被运行。钩子从不下载任何东西。要改用本地构建，请设置 `PROOB_BIN`。

前提条件：

- **GitHub 读取权限**，用于得到经过验证的状态：`gh auth login`，或在环境中设置 `GITHUB_TOKEN`。没有它，每个状态都会标记为 `unverified`，也不会出现 `Holds`。
- **Python 项目：** 在运行测试的环境中安装 pytest 9.1.1、Hypothesis 6.169.4 和 mutmut 3.8.0（[`tools/python-tools.txt`](tools/python-tools.txt)）。
- **Rust 项目：** cargo-nextest 和 cargo-mutants。走证明路线时还需要固定版本的 Verus，可用 [`tools/get-verus.sh`](tools/get-verus.sh) 获取。

## 使用

### 不改动仓库，先试一试

```text
/proob:try
```

在 Claude Code 中运行这一条命令，它会阅读你的代码、提出性质、为每条性质写一个测试、运行测试、衡量每个测试是否真的能失败，最后打印一份报告。这一切都发生在 `~/.proob/shadow/` 下的一个私有副本中：你的文件、你的 `.git` 和 `git status` 都保持原样。副本基于你最近一次提交，所以想纳入的改动请先提交。

报告的最后会针对你的仓库写出把它变成正式流程的步骤：

1. `proob shadow adopt` 把性质、对应的测试、配置和两个工作流放到一个新分支上，不会切换你当前的检出。
2. 推送该分支并创建拉取请求。由队友审查即为批准。
3. 在默认分支上把 `proob gate` 检查设为必需。

在此之前，每条性质都显示 `Proposed`，因为还没有任何人批准过任何内容。`proob shadow clean` 会删除这个副本。

### 正式采用

在 Claude Code 中打开你的仓库，按顺序运行这些技能：

```text
/proob:sense      read the code; write proob's picture of it to a local cache
/proob:declare    propose properties and open a pull request for your team
/proob:falsify    write one test per approved property and run it
/proob:verify     measure whether each test can fail; explain the survivors
/proob:guard      show where the team stands and what is missing
```

第一次运行 `/proob:declare` 时也会执行 `proob init`，因此它的拉取请求会带上 `proob/config.toml` 和两个运行检查的工作流。审查者只用 GitHub 就能批准：

1. 合并即批准该拉取请求中每个文件的当前内容。只有作者以外的人的审查才算团队批准；没有这样的审查而直接合并属于自签名，proob 会如实标出。
2. 想修改措辞，请在相应行上使用 GitHub 的 "suggest"，由作者提交该建议。你也可以自己推送修改，但要知道，你修改过的文件随后需要其他人批准。
3. 想只拒绝其中一条提议，请在该拉取请求中回复 `reject <ULID>`（填入那条提议的 id），或在拉取请求中删除那个文件。

每个状态都使用以下七个词之一：

| 词 | 含义 | 该做什么 |
|---|---|---|
| Holds | 陈述、检查和配置都已批准；证据为正；检查已被证明能够失败 | 无需操作 |
| Checked | 同上，但强度尚未测量，或属于自签名 | 等待夜间运行，或找第二位审查者 |
| Not checked | 已批准，但没有检查、没有适配器、没有近期证据，或无法访问 GitHub | 绑定一个检查，或等待下一次运行 |
| Re-approve | 陈述、检查、配置或工作流在批准后发生了变化 | 审查这次修订 |
| Broken | 本拉取请求上出现反例或证明失败、范围为空，或豁免已过期 | 修复代码，或修订陈述 |
| Proposed | 从未被批准 | 审查并合并，或拒绝 |
| Waived until d | 已批准但失败，有人工豁免和一个未关闭的 issue | 跟踪该 issue |

在一个真实服务上的完整演示，附有二进制输出的每一行：[`examples/python-service/`](examples/python-service/)。

### 当需求变化时

在开发该功能的分支上运行 `/proob:declare`。它会根据你所处的情况判断需要做什么：

- 在功能分支上，它会读取该分支的改动。对于没有任何性质覆盖的新代码，它会提出性质并提交到同一个分支，这样审查者会把功能和它的性质一起批准。
- 如果你的改动因为需求本身变了而破坏了某条已有性质，它会写一条替代性质，并说明它取代的是哪一条。审查者批准新的那条并删除旧的。如果错的是代码，它会直接指出，不提出任何性质。
- 给它一段文字、一个 issue 编号或一个 URL，例如 `/proob:declare refunds never exceed the original charge`，它会为这个需求提出性质。

你不需要记着去运行它。当拉取请求加入了没有任何性质覆盖的代码时，proob gate 检查会在 "New code without a property" 下列出这些文件，并建议运行 `/proob:declare`；它从不阻塞合并。你工作时，会话开头的那行状态也会这样提示。要把某些路径排除在这个列表之外，请在 `proob/config.toml` 的 `uncovered_ignore` 中加入 glob。

### 命令行

这些技能驱动 `proob` 二进制，你也可以直接运行它：

```sh
proob validate              # check proob's files and binding tokens
proob status                # derive every property's status
proob check --base main --head HEAD   # the gate: status plus policy for this change
proob run check <id>        # run one property's bound check locally
proob run score <id>        # measure whether that check can fail
proob shadow open           # make or refresh the private copy /proob:try uses
proob shadow report         # what was tried, what failed, how to make it real
proob shadow adopt          # put the shadow's work on a new branch of your repository
proob --shadow <command>    # run any command above against the private copy
```

## 基准测试

每个版本都用同样的方法衡量，这些方法取自软件测试与验证领域的既有实践，而不是我们自己发明的（[方法说明](bench/README.md)）：

- **发现的真实缺陷。** 以开发者自己的修复为准，与 Defects4J 和 BugsInPy 相同：只有在应用修复后消失的失败才算数。
- **找到缺陷的时间。** 每个缺陷使用许多个独立种子，以分布形式报告，遵循 Klees 等人的 "Evaluating Fuzz Testing"（CCS 2018）。
- **结论的可靠性。** 用 SV-COMP 的计分方案为结论打分，错误的 `Holds` 扣分是正确 `Holds` 得分的十六倍。

v0.0.1 的结果（[完整报告](bench/results/v0.0.1.md)），基于 11 个库中由开发者修复的 11 个缺陷，每个缺陷做 3 次不知情的试验：

| 指标 | 结果 |
|---|---|
| 至少被发现一次的缺陷 | 11 个中 10 个 |
| 命中目标缺陷的试验 | 33 次中 22 次，67%（95% 区间 50% 至 80%） |
| 已绑定的检查找到缺陷所需时间 | 按缺陷计，中位数 0.3 秒至 8.3 秒 |
| 报告的失败中属于真实缺陷的比例 | 44 个中 39 个，89% |
| 在当前发布版本中发现的新缺陷 | 5 个，分布在 python-semver、rust-url 和 tomlkit 中 |
| 完整智能体流程的成本 | 每次试验中位数 7 分钟、1.85 美元 |

涉及的库：more-itertools、tomlkit、humanize、cachetools、python-semver、packaging、bidict（Python）；chrono、unicode-segmentation、rust-url、toml（Rust）。唯一未发现的是 bidict，三次试验都没有命中。所有修复都发生在 2025 至 2026 年，智能体所用的模型可能见过其中一些；下一版缺陷集会加入在其训练截止日期之后修复的缺陷。

在知名代码库上（[目标基准测试](bench/results/targets-v1.md)）：以新用户的方式运行 `/proob:try`，对象是固定提交上的 attrs 和 rich（Python）以及 indexmap 和 vaultwarden（Rust）。

| 指标 | 结果 |
|---|---|
| 提出的性质中得到可运行测试的 | 35 个中 35 个，共 7 次完整试验 |
| 发现的真实缺陷，在最新发布版本中仍然存在 | 7 个不同缺陷：attrs 3 个，rich 3 个，vaultwarden 两步验证登录路径中 1 个 |
| 报告的失败中属于真实缺陷的 | 15 个中 11 个；3 个误报，1 个有争议 |
| 试验改动了仓库 | 从未：逐字节一致，包括 `.git` |
| 成本 | 每次试验中位数 1.10 美元 |

本版本中 Rust 目标的数据不完整：变异评分逐个运行每个变异体，indexmap 每条性质需要 35 至 40 分钟。下一轮迭代会先提升速度，然后完整重跑。

proob 的每一项改动都会在缺陷集的开发集上与上一版本对比，只有在没有显著退化时才会发布；保留集在每次发布时报告。每一次命中也会成为回归测试，CI 在每次改动时都会重放，不需要任何模型参与（[方法说明](bench/README.md)）。

| 迭代 | 改动 | 开发集（6 个缺陷） | 保留集（5 个缺陷） |
|---|---|---|---|
| v0.0.1 | 基线 | 18 次试验中 10 次 | 15 次试验中 12 次 |
| dev-1 | 写出标准接口的定律（映射、迭代器、相等与哈希、往返转换） | 18 次试验中 14 次；所针对的两个缺陷从 6 次中 1 次提升到 6 次中 6 次 | 将在下一次发布时测量 |
| v0.1.0（[结果](bench/results/v0.1.0.md)） | dev-1，加上抽样且并行的变异评分、cargo features | 未重跑 | 10 次试验中 9 次，另有截止日期之后的 10 个缺陷中的 9 个 |

[v0.1.0-rc1 的结果](bench/results/v0.1.0-rc1.md)增加了一项更严格的检验：**10 个在智能体模型训练截止日期之后才修复的真实缺陷，模型不可能见过它们。proob 每个缺陷试验一次，发现了其中 9 个。** 在所有保留缺陷上，20 次试验中有 18 次命中（90%，95% 区间 70% 至 97%）。在 260 个生成的结论上按 SV-COMP 方式计分，proob 从未在代码并不成立时给出 `Holds`。indexmap 上一次强度评分现在只需 66 秒，而不是 700 秒。

## 我们如何确认它可靠

插件和 Action 中没有任何代码路径能因为智能体的操作而让一条性质显示为已批准。批准在读取时从 GitHub 的审查记录中推导。证据只来自默认分支上的 Action 运行，而且运行检查的那个作业不持有任何令牌。Action 没有任何写权限。钩子只是在此之上的反馈，出错时一律放行。

做出判定的函数是 crate `proob-core` 中的 `verdict`。做出判定的 crate 在 `--no-cheating` 下由 Verus 验证；可信基础在此：[`crates/proob-trusted/README.md`](crates/proob-trusted/README.md)，只有两个函数，`sha256` 和 `now_secs`。六条断言定义了 `Holds`，每一条都是在每个拉取请求上检查的 `ensures` 子句（[`ASSERTIONS.md`](crates/proob-core/ASSERTIONS.md)）：

1. 只有当性质的当前内容经过合格审查批准时才是 `Holds`；自签名的合并永远不算。
2. 只有当绑定的检查、`proob/config.toml` 和证据工作流各自都经过合格审查批准时才是 `Holds`。
3. 只有当范围能匹配到文件，且默认分支上最新的证据在当前哈希下既新鲜又为正时才是 `Holds`。
4. 只有当新鲜的评分表明检查杀死的变异体不少于下限时才是 `Holds`。
5. 只有在豁免已批准、issue 未关闭且期限未过时才是 `Waived`。
6. 合格性判定是可靠的，直接推送永远不算批准，任何结论都不会作为输入。

对于用 Verus 验证的 Rust，强度通过"证明下的变异"来衡量：proob 修改证明之下的代码，然后再次运行验证器。如果证明仍然接受这处改动，说明规约分不清对错。

`cargo xtask sim` 针对 GitHub、发布服务器和 Claude Code 钩子的本地替身运行整个生命周期，并检查一组对抗性场景永远不会显示 `Holds`。

## 支持的语言

| 语言 | 检查 | 强度 |
|---|---|---|
| Rust | proptest，通过 cargo-nextest | cargo-mutants |
| 用 Verus 验证的 Rust | 验证器，作用于标有该性质 id 的条目 | 证明下的变异 |
| Python | Hypothesis，通过 pytest | mutmut |
| TypeScript | 发布后：fast-check，通过 vitest | StrykerJS |
| Go、Java | 计划中 | |

在其他语言中，sense 和 declare 依然可用，每条性质会显示 `Not checked (no adapter)`。

## 团队使用

`proob init` 会提交一个 `.claude/settings.json`，向每个在 Claude Code 中打开该仓库的人推荐这个插件。队友信任该文件夹后安装一次插件（`/plugin install proob@proob`），之后每次会话开始时都会看到一行状态。审查和批准完全不需要插件。更多内容：[`skills/guard/reference/team-install.md`](skills/guard/reference/team-install.md)。

我们建议在默认分支上：

- 要求 `proob gate` 检查通过，并要求分支保持最新，或使用合并队列；
- 合并前至少需要一次批准性审查，使未经审查的内容无法进入分支；
- 为 `proob/` 和两个 proob 工作流文件添加 `CODEOWNERS` 条目。

`/proob:guard` 会报告其中哪些已经就位。

## 局限

- 本仓库只有一位维护者，所以它自身的性质最多只能显示 `Checked (self-signed)`。
- 测试这些技能的评测需要 API 密钥，而且每次运行的分数会有波动，因此不是必需的检查。
- 对于公开仓库，GitHub 会在 60 天无活动后停用定时工作流。夜间运行停止后，评分会过期，`Holds` 会降为 `Checked`。
- mutmut 需要 `os.fork`，因此 Python 检查只能在 Linux 和 macOS 上评分。
- 暂不支持 TLA+。

## 参与贡献

问题和想法请发到 [Discussions](https://github.com/proob-ai/proob/discussions)，缺陷报告请提交到 [GitHub issues](https://github.com/proob-ai/proob/issues)，安全漏洞请通过私密安全公告报告（[`SECURITY.md`](SECURITY.md)）。欢迎向 `develop` 分支提交拉取请求。[`CONTRIBUTING.md`](CONTRIBUTING.md) 介绍了分支、提交信息、经过验证的核心，以及涉及检测的变更所须遵守的规则。提交前请运行与 CI 相同的检查：

```sh
cargo xtask ci
```

[`xtask/README.md`](xtask/README.md) 列出了所有命令及其前提条件。变更记录在 [`CHANGELOG.md`](CHANGELOG.md) 中。本 README 的各语言版本以英文版为准：修改 `README.md` 后，请更新每个 `README.<language>.md` 及其源哈希，`cargo xtask readme-sync` 会确认它们一致。所有参与者都须遵守[行为准则](CODE_OF_CONDUCT.md)。

## 许可证

[Apache-2.0](LICENSE) © proob-ai
