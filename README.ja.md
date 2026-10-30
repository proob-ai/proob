<!-- translated from README.md sha256:efb2a7b8cd3f8bca877ed969d8ae05077fb6d23d7649a468e0f39b0e49b269e7 -->
# proob

[![ci](https://github.com/proob-ai/proob/actions/workflows/ci.yml/badge.svg)](https://github.com/proob-ai/proob/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

[English](README.md) · [简体中文](README.zh-CN.md) · **日本語**

**テストは誰かが思いついたケースしか確かめません。proob は、誰もテストしようと思わなかったバグを見つけます。**

proob は Claude Code のプラグインです。コードを読み、常に成り立つべきルールを平易な言葉で書き出し（たとえば「返金額は元の請求額を決して超えない」）、自動生成した何千もの入力で各ルールを攻撃します。チームは通常のプルリクエストのレビューでルールを承認し、以後 proob はすべてのプルリクエストでそれを再検査します。エージェントは提案するだけで、承認できるのはチームだけです。

```text
$ claude   # python-semver で、メンテナーが issue #497 を修正する一つ前のコミット
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
```

実際の実行の抜粋です。この失敗は、python-semver のメンテナーが後に修正したバグそのものです。次のプレリリース版を求めると、バージョンが逆戻りすることがありました。コマンド一つ、約 5 分と 1.10 ドルで、リポジトリのファイルは一つも変わっていません。

## 本当に効くのか

各リリースを、実際のテストスイートをすり抜けた実際のバグで測定しています。手法はテスト研究（Defects4J、Klees ら CCS 2018、SV-COMP）から借りたもので、独自のものではありません。

| テスト | 結果 |
|---|---|
| モデルの学習カットオフ**後**に修正された実バグ（記憶しようがない） | **10 件中 9 件を検出** |
| ホールドアウトした過去のバグ、ブラインド試行 | **20 試行中 18 試行**（90%） |
| attrs、rich、vaultwarden、python-semver、rust-url、tomlkit の当時の最新リリースで見つかった未知のバグ | **12 件** |
| 採点した 260 件の判定のうち、コードがルールを破っているのに `Holds` と判定した回数 | **0** |
| 報告した失敗のうち実際のバグだった割合 | 59 件中 50 件（85%） |

検出はそれぞれ回帰テストになり、CI がモデルなしで再生します。紐付いたチェックは 10 秒以内にそのバグを再発見します。詳しい数値：[docs/benchmark.md](docs/benchmark.md)。

## クイックスタート

Claude Code で、あなたのリポジトリの中で：

```text
/plugin marketplace add proob-ai/proob
/plugin install proob@proob
/proob:try
```

`/proob:try` は `~/.proob/shadow/` 以下の非公開コピーで動くため、あなたのファイル、`.git`、`git status` はそのままです。pytest、Hypothesis、mutmut を入れた Python、または cargo-nextest と cargo-mutants を入れた Rust が必要です（[詳細](docs/guide.md#install)）。

結果が気に入ったら、レポートの最後にそれを残す 3 つの手順があります：

1. `proob shadow adopt` がルール、テスト、2 つのワークフローを新しいブランチに置きます。
2. プルリクエストを開きます。チームメイトのレビューが承認です。
3. デフォルトブランチで `proob gate` チェックを必須にします。

## 仕組み

| スキル | 内容 |
|---|---|
| `/proob:sense` | エージェントがコードを読む |
| `/proob:declare` | ルールを平易な言葉のファイルとして提案し、チームがレビューするプルリクエストに入れる |
| `/proob:falsify` | 承認された各ルールにプロパティベーステストを書き、固定シードで実行する |
| `/proob:verify` | proob がコードを変異させ、各テストが実際に失敗しうることを確かめる |
| `/proob:guard` | プルリクエストごと、毎晩、セッションごとに、どのルールがまだ成り立つかを示す |

各ルールの状態は `Holds`、`Checked`、`Not checked`、`Re-approve`、`Broken`、`Proposed`、`Waived` のいずれかです。`Holds` は、作成者以外のレビュアーが承認し、デフォルトブランチからの新しい証拠があり、失敗しうることが確かめられたテストで検査されていることを意味します。レビューするだけのチームメイトには GitHub だけで足ります。各スキル、状態、チーム設定、CLI は[ガイド](docs/guide.md)（英語）にあります。

## 判定を信頼できる理由

- **エージェントは何も承認できません。** 状態を表示するたびに GitHub のレビュー記録から承認を読み取ります。リポジトリには承認を保存せず、自分でマージした変更は決して数えません。
- **証拠はデフォルトブランチからのみ、** トークンを持たないジョブから得ます。
- **判定するコードは形式検証済みです。** 判定関数は [Verus](https://github.com/verus-lang/verus) で、`Holds` を定義する [6 つの表明](crates/proob-core/ASSERTIONS.md)に対して証明されています。信頼される基盤は[2 つの関数](crates/proob-trusted/README.md)だけです。

## 対応言語

| 言語 | チェック | 強度 |
|---|---|---|
| Python | pytest 上の Hypothesis | mutmut |
| Rust | cargo-nextest 上の proptest | cargo-mutants |
| Verus 下の Rust | 検証器 | 証明下の変異 |
| TypeScript | 予定：vitest 上の fast-check | StrykerJS |

その他の言語でも sense と declare は使え、各ルールは `Not checked (no adapter)` と表示されます。既知の制限は[ガイド](docs/guide.md#limits)にあります。

## コントリビュート

質問は [Discussions](https://github.com/proob-ai/proob/discussions) へ、バグは [issues](https://github.com/proob-ai/proob/issues) へ、脆弱性は非公開のセキュリティアドバイザリへ（[`SECURITY.md`](SECURITY.md)）。`master` へのプルリクエストを歓迎します。[`CONTRIBUTING.md`](CONTRIBUTING.md) がチェックを説明し、`cargo xtask ci` がすべてを実行します。英語の README を編集したら、各 `README.<language>.md` とそのソースハッシュを更新してください。`cargo xtask readme-sync` が一致を確認します。

## ライセンス

[Apache-2.0](LICENSE) © proob-ai
