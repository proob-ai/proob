<!-- translated from README.md sha256:765305705cb20d4edf2d6cdc5b5ac9b9bd9e7ce02c2e78eefc8256b664465ba1 -->
# proob

[![ci](https://github.com/proob-ai/proob/actions/workflows/ci.yml/badge.svg)](https://github.com/proob-ai/proob/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

[English](README.md) · [简体中文](README.zh-CN.md) · **日本語**

チームが承認し、機械が検査し、継続的に守られるコードの性質。

コードが決してしてはならないことを、チームが平易な言葉で取り決めます。proob はそれをすべてのプルリクエストで検査し、成り立たなくなったその日に知らせます。エージェントがコードを読んで記述を提案し、チームはプルリクエストのレビューでそれを承認します。そして、すべての判定は既定ブランチで実際に実行されたテストと証明から得られます。

**有名なオープンソースライブラリのテストをすり抜けた実在のバグ 11 件に対し、バグを知らされていない proob のエージェントは 10 件を発見しました。33 回の試行のうち 22 回で的中しています。さらに、そのモデルの学習データの締め切り後に修正された 10 件のうち 9 件を見つけました。** エージェントが書いたチェックは、どのバグも 10 秒以内に再び見つけます。attrs、rich、vaultwarden では、最新リリースにも残っている実在のバグを 7 件見つけました。詳しくは[ベンチマーク](#ベンチマーク)を参照してください。まずはリポジトリを変えずに、自分のコードで何が見つかるかを確かめたいなら [`/proob:try`](#リポジトリに手を加えずに試す) を実行してください。

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

実際の `/proob:try` の実行（抜粋）：コマンド 1 つ、約 5 分、1.10 ドル。proob は python-semver のメンテナーが後に修正したバグ（次のプレリリースを求めると後退することがある。たとえば `1.2.3-rc.1` にトークン `alpha` で `1.2.3-alpha.1` になる）を見つけ、リポジトリのファイルは一つも変えませんでした。

## 目次

- [背景](#背景)
- [インストール](#インストール)
- [使い方](#使い方)
- [ベンチマーク](#ベンチマーク)
- [動作を確かめる仕組み](#動作を確かめる仕組み)
- [対応言語](#対応言語)
- [チームでの利用](#チームでの利用)
- [制限事項](#制限事項)
- [コントリビュート](#コントリビュート)
- [ライセンス](#ライセンス)

## 背景

テストは、誰かがテストしようと思いついたことしか確かめません。形式手法は常に成り立つべきことを確かめられますが、専門家を必要とし、コードから取り残されがちです。proob は本当に大切な部分、つまりチームが合意した短い性質のリストだけを残し、残りはすべて機械に任せます。

| 段階 | 担い手 | 内容 |
|---|---|---|
| Sense（把握） | エージェント | コードを読み、システムの理解をローカルキャッシュに書き出す |
| Declare（宣言） | エージェント、次に人 | 平易な言葉で書いた性質をファイルとして提案し、チームがプルリクエストで承認する |
| Falsify（反証） | エージェント、次に機械 | 性質ごとにプロパティベーステストを 1 つ書き、proob が固定のシードと予算で実行する |
| Verify（検証） | 機械 | 対象コードを変異させ、各チェックが本当に失敗しうるかを測る |
| Guard（監視） | 機械 | すべてのプルリクエスト、毎晩、そしてセッション開始のたびに、すべての判定を導き直す |

エージェントは提案するだけで、決して承認しません。状態を表示するたびに、承認は GitHub のレビュー記録から読み取られます。リポジトリには承認を一切保存しません。

proob 自身の状態です。このリポジトリのレビュー記録と既定ブランチでの実行結果から、その場で生成しています。

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

テストに裏付けられた 4 つの性質は `Checked (self-signed)` です。既定ブランチで検査済みですが、このリポジトリのメンテナーは 1 人なので、2 人目のレビューなしで承認されており、最初の夜間強度スコアを待っています。証明に裏付けられた 9 つの性質は `Re-approve` です。レビューを経たプルリクエストを通さないコミットが、証明の依存するファイルを変更したためです。これはルールが働いている証拠です。誰もレビューしていない変更は承認を保てません。`Holds` には 2 人目のメンテナーが必要で、その人が現れるまでこの表に `Holds` は表示されません。

## インストール

Claude Code で次を実行します。

```text
/plugin marketplace add proob-ai/proob
/plugin install proob@proob
```

Claude Code 2.1.275 以降では 1 回で済みます。

```text
/plugin install proob --marketplace proob-ai/proob
```

スキルを初めて実行すると、プラグインはこのリポジトリの GitHub Releases から、お使いのプラットフォーム向けの `proob` バイナリをダウンロードします。公開されている `.sha256` ファイルで SHA-256 を照合し、リリースに証明が付いていて `gh` にログインしていれば Sigstore の証明も確認したうえで（このリポジトリが非公開の間のリリースには付きません）、`~/.proob/bin/` にキャッシュします。チェックサムが一致しないファイルは決して実行されません。フックは何もダウンロードしません。ローカルビルドを使う場合は `PROOB_BIN` を設定してください。

前提条件：

- **GitHub の読み取り権限**（検証済みの状態を得るため）：`gh auth login`、または環境変数 `GITHUB_TOKEN`。これがないと、すべての状態に `unverified` が付き、`Holds` は表示されません。
- **Python プロジェクト：** テストを実行する環境に pytest 9.1.1、Hypothesis 6.169.4、mutmut 3.8.0（[`tools/python-tools.txt`](tools/python-tools.txt)）。
- **Rust プロジェクト：** cargo-nextest と cargo-mutants。証明ルートには固定版の Verus が必要で、[`tools/get-verus.sh`](tools/get-verus.sh) で取得できます。

## 使い方

### リポジトリに手を加えずに試す

```text
/proob:try
```

Claude Code でこのコマンドを 1 つ実行するだけで、コードを読み、性質を提案し、それぞれにテストを書いて実行し、各テストが本当に失敗しうるかを測り、レポートを表示します。すべて `~/.proob/shadow/` 以下の専用コピーの中で行われ、あなたのファイルも `.git` も `git status` もそのままです。コピーは最新のコミットをもとにするので、含めたい変更は先にコミットしてください。

レポートの最後には、あなたのリポジトリに合わせて、本番の流れに組み込む手順が書かれています。

1. `proob shadow adopt` が、性質とそのテスト、設定、2 つのワークフローを新しいブランチに置きます。チェックアウト中のブランチは切り替えません。
2. そのブランチをプッシュしてプルリクエストを作ります。チームメイトのレビューが承認になります。
3. 既定ブランチで `proob gate` チェックを必須にします。

それまで、すべての性質は `Proposed` と表示されます。まだ誰も何も承認していないからです。`proob shadow clean` でコピーを削除できます。

### 本格的に導入する

Claude Code でリポジトリを開き、スキルを順に実行します。

```text
/proob:sense      read the code; write proob's picture of it to a local cache
/proob:declare    propose properties and open a pull request for your team
/proob:falsify    write one test per approved property and run it
/proob:verify     measure whether each test can fail; explain the survivors
/proob:guard      show where the team stands and what is missing
```

最初の `/proob:declare` では `proob init` も実行されるため、そのプルリクエストには `proob/config.toml` と、チェックを実行する 2 つのワークフローが含まれます。レビュアーは GitHub だけで承認できます。

1. マージすると、このプルリクエスト内のすべてのファイルが現在の内容で承認されます。作成者以外によるレビューだけがチームの承認として数えられます。そのようなレビューのないマージは自己署名となり、proob はそう表示します。
2. 文言を変えたい場合は、該当行で GitHub の "suggest" を使い、作成者にその提案をコミットしてもらってください。自分で修正をプッシュしてもかまいませんが、その場合、あなたが編集したファイルには別の人の承認が必要になります。
3. 提案のうち 1 つだけを却下するには、このプルリクエストにその提案の id を付けて `reject <ULID>` と返信するか、プルリクエスト内でそのファイルを削除してください。

状態は必ず次の 7 語のいずれかで表されます。

| 語 | 意味 | すること |
|---|---|---|
| Holds | 記述・チェック・設定が承認済み、証拠が肯定的、チェックが失敗しうることも確認済み | なし |
| Checked | 上と同じだが、強度が未測定、または自己署名 | 夜間実行を待つか、2 人目のレビュアーを得る |
| Not checked | 承認済みだが、チェック・アダプター・最近の証拠のいずれかがない、または GitHub に接続できない | チェックを結び付けるか、次の実行を待つ |
| Re-approve | 承認後に記述・チェック・設定・ワークフローが変更された | その修正をレビューする |
| Broken | このプルリクエストで反例または証明の失敗が出た、スコープが空、または免除が期限切れ | コードを直すか、記述を修正する |
| Proposed | 一度も承認されていない | レビューしてマージするか、却下する |
| Waived until d | 承認済みで失敗中だが、人による免除と未解決の issue がある | その issue を追跡する |

実際のサービスでの手順を、バイナリの出力をすべて載せて示しています：[`examples/python-service/`](examples/python-service/)。

### 要件が変わったとき

機能を開発しているブランチで `/proob:declare` を実行します。今いる状況から、何が必要かを判断します。

- 機能ブランチでは、そのブランチの変更を読みます。どの性質にも覆われていない新しいコードについて性質を提案し、同じブランチにコミットします。レビュアーは機能とその性質をまとめて承認できます。
- 要件そのものが変わったために既存の性質を壊す変更なら、置き換えとなる性質を書き、どれを置き換えるかを明記します。レビュアーは新しいほうを承認し、古いほうを削除します。誤っているのがコードのほうなら、そう指摘し、何も提案しません。
- `/proob:declare refunds never exceed the original charge` のように文章、issue 番号、URL を渡すと、その要件について性質を提案します。

実行を覚えておく必要はありません。どの性質にも覆われていないコードをプルリクエストが追加すると、proob gate チェックが "New code without a property" の下にそれを挙げ、`/proob:declare` を勧めます。マージを止めることはありません。作業中はセッション開始時の 1 行も同じことを知らせます。この一覧から除外したいパスは、`proob/config.toml` の `uncovered_ignore` に glob で指定してください。

### CLI

スキルは `proob` バイナリを動かしています。直接実行することもできます。

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

## ベンチマーク

各リリースは同じ方法で測定します。その方法は私たちが考案したものではなく、ソフトウェアテストと検証の分野で確立されたものです（[方法](bench/README.md)）。

- **実在のバグの検出。** 正解は開発者自身による修正で、Defects4J や BugsInPy と同じ考え方です。修正を当てると消える失敗だけを数えます。
- **バグ発見までの時間。** バグごとに多数の独立したシードで実行し、分布として報告します。Klees らの "Evaluating Fuzz Testing"（CCS 2018）に従っています。
- **判定の健全性。** SV-COMP の採点方式で判定を採点します。誤った `Holds` の減点は、正しい `Holds` の得点の 16 倍です。

v0.0.1 の結果（[完全なレポート](bench/results/v0.0.1.md)）。11 のライブラリにある、開発者が修正したバグ 11 件を対象に、バグを知らせない試行を各 3 回行いました。

| 指標 | 結果 |
|---|---|
| 少なくとも 1 回見つかったバグ | 11 件中 10 件 |
| 対象バグを見つけた試行 | 33 回中 22 回、67%（95% 区間 50%〜80%） |
| 結び付けたチェックがバグを見つけるまでの時間 | バグごとの中央値で 0.3 秒〜8.3 秒 |
| 報告された失敗のうち実在のバグだったもの | 44 件中 39 件、89% |
| 現行リリースで見つかった新しいバグ | 5 件（python-semver、rust-url、tomlkit） |
| エージェントの全工程のコスト | 試行あたり中央値 7 分、1.85 ドル |

対象ライブラリ：more-itertools、tomlkit、humanize、cachetools、python-semver、packaging、bidict（Python）、chrono、unicode-segmentation、rust-url、toml（Rust）。見つからなかった 1 件は bidict で、3 回とも見逃しました。修正はすべて 2025〜2026 年のもので、エージェントのモデルがその一部を見ている可能性があります。次のバグセットには、学習データの締め切り後に修正されたバグを加えます。

有名なコードベースでの結果（[ターゲットベンチマーク](bench/results/targets-v1.md)）：新しいユーザーと同じように `/proob:try` を、固定したコミットの attrs と rich（Python）、indexmap と vaultwarden（Rust）で実行しました。

| 指標 | 結果 |
|---|---|
| 提案された性質のうち、動くテストが付いたもの | 35 件中 35 件（完了した試行 7 回） |
| 見つかった実在のバグで、最新リリースにも残っているもの | 7 件：attrs に 3 件、rich に 3 件、vaultwarden の二要素認証ログイン経路に 1 件 |
| 報告された失敗のうち実在のバグだったもの | 15 件中 11 件。誤検知 3 件、判断の分かれるもの 1 件 |
| 試行がリポジトリを変えたこと | 一度もない：`.git` を含めバイト単位で同一 |
| コスト | 試行あたり中央値 1.10 ドル |

このバージョンでは Rust のターゲットのデータが不完全です。変異スコアの計算が変異体を 1 つずつ実行するため、indexmap では性質ごとに 35〜40 分かかりました。次の改善で速度を上げてから、すべてやり直します。

proob への変更は、バグセットの開発用の半分で前のバージョンと比べて測り、有意な後退がない場合にだけリリースします。保留した半分は各リリースで報告します。見つけたバグはそれぞれ回帰テストにもなり、CI が変更のたびにモデルを使わず再実行します（[方法](bench/README.md)）。

| 改善 | 変更内容 | 開発用（6 件） | 保留（5 件） |
|---|---|---|---|
| v0.0.1 | 基準 | 18 回中 10 回 | 15 回中 12 回 |
| dev-1 | 標準インターフェースの法則（マッピング、イテレーター、等価とハッシュ、往復変換）を書き出す | 18 回中 14 回。狙った 2 件は 6 回中 1 回から 6 回中 6 回に | 次のリリースで測定 |
| v0.1.0（[結果](bench/results/v0.1.0.md)） | dev-1 に加え、変異体のサンプリングと並列実行、cargo features | 再実行せず | 10 回中 9 回、締め切り後のバグ 10 件中 9 件 |

[v0.1.0-rc1 の結果](bench/results/v0.1.0-rc1.md)では、より厳しい検証を加えました。**エージェントのモデルの学習データの締め切り後に修正された実在のバグ 10 件、つまりモデルが見たはずのないバグに対し、各 1 回の試行で 9 件を見つけました。** 保留したすべてのバグでは、20 回中 18 回で的中しました（90%、95% 区間 70%〜97%）。生成した 260 個の判定を SV-COMP 方式で採点したところ、コードが成り立っていないのに `Holds` と言ったことは一度もありません。indexmap の強度スコアは 700 秒から 66 秒になりました。

## 動作を確かめる仕組み

プラグインにも Action にも、エージェントの操作によって性質を承認済みとして表示させるコード経路はありません。承認は読み取りのたびに GitHub のレビュー記録から導かれます。証拠は既定ブランチ上の Action 実行からのみ得られ、チェックを動かすジョブはトークンを一切持ちません。Action には書き込み権限が一切ありません。フックはその上に乗るフィードバックにすぎず、失敗しても作業を止めません。

判定を下す関数は、crate `proob-core` の `verdict` です。判定を下す crate は `--no-cheating` のもとで Verus により検証されています。信頼の基盤はこちらです：[`crates/proob-trusted/README.md`](crates/proob-trusted/README.md)。関数は `sha256` と `now_secs` の 2 つだけです。6 つの表明が `Holds` を定義しており、それぞれがすべてのプルリクエストで検査される `ensures` 節です（[`ASSERTIONS.md`](crates/proob-core/ASSERTIONS.md)）。

1. 性質の現在の内容が適格なレビューで承認されたときに限り `Holds`。自己署名のマージは数えない。
2. 結び付けたチェック、`proob/config.toml`、証拠ワークフローのそれぞれが適格なレビューで承認されたときに限り `Holds`。
3. スコープがファイルに一致し、既定ブランチの最新の証拠が現在のハッシュで新しく、かつ肯定的なときに限り `Holds`。
4. 新しいスコアで、チェックが下限以上の変異体を検出したときに限り `Holds`。
5. 免除が承認済みで、issue が未解決で、期限内のときに限り `Waived`。
6. 適格性の判定は健全であり、直接のプッシュは決して承認にならず、いかなる判定も入力にならない。

Verus で検証された Rust では、強度を「証明下の変異」で測ります。proob は証明の下にあるコードを変更し、検証器をもう一度実行します。証明がその変更をなお受け入れるなら、その仕様は正しいものと誤ったものを区別できていません。

`cargo xtask sim` は、GitHub、リリースサーバー、Claude Code のフックをローカルの代役に置き換えてライフサイクル全体を実行し、一連の敵対的なケースで `Holds` が決して表示されないことを確かめます。

## 対応言語

| 言語 | チェック | 強度 |
|---|---|---|
| Rust | proptest（cargo-nextest 経由） | cargo-mutants |
| Verus で検証された Rust | 性質の id を付けた項目に対する検証器 | 証明下の変異 |
| Python | Hypothesis（pytest 経由） | mutmut |
| TypeScript | リリース後：fast-check（vitest 経由） | StrykerJS |
| Go、Java | 予定 | |

それ以外の言語でも sense と declare は動作し、各性質は `Not checked (no adapter)` と表示されます。

## チームでの利用

`proob init` は `.claude/settings.json` をコミットし、このリポジトリを Claude Code で開くすべての人にプラグインを推奨します。チームメイトはフォルダーを信頼し、プラグインを一度インストールすれば（`/plugin install proob@proob`）、以後はセッション開始時に 1 行の状態が表示されます。レビューと承認にはプラグインは一切不要です。詳しくは [`skills/guard/reference/team-install.md`](skills/guard/reference/team-install.md) を参照してください。

既定ブランチでは次の設定をお勧めします。

- `proob gate` チェックを必須にし、ブランチを最新に保つことを必須にするか、マージキューを使う。
- マージ前に承認レビューを 1 件以上必須にし、レビューされていない内容がブランチに入らないようにする。
- `proob/` と 2 つの proob ワークフローファイルに `CODEOWNERS` を設定する。

`/proob:guard` は、これらのうちどれが整っているかを報告します。

## 制限事項

- このリポジトリのメンテナーは 1 人なので、自身の性質は最高でも `Checked (self-signed)` にしかなりません。
- スキルを試す評価には API キーが必要で、実行ごとにスコアが変動するため、必須のチェックにはしていません。
- 公開リポジトリでは、60 日間活動がないと GitHub が定期ワークフローを無効にします。夜間実行が止まるとスコアが古くなり、`Holds` は `Checked` に下がります。
- mutmut は `os.fork` を必要とするため、Python のチェックの採点は Linux と macOS でのみ可能です。
- TLA+ にはまだ対応していません。

## コントリビュート

質問やアイデアは [Discussions](https://github.com/proob-ai/proob/discussions) へ、バグ報告は [GitHub issues](https://github.com/proob-ai/proob/issues) へ、脆弱性は非公開のセキュリティアドバイザリで報告してください（[`SECURITY.md`](SECURITY.md)）。プルリクエストは `master` ブランチ宛てで歓迎します。ブランチ、コミットメッセージ、検証済みのコア、検出に関わる変更のルールは [`CONTRIBUTING.md`](CONTRIBUTING.md) にまとめています。送る前に、CI と同じチェックを実行してください。

```sh
cargo xtask ci
```

すべてのコマンドとその前提条件は [`xtask/README.md`](xtask/README.md) にあります。変更履歴は [`CHANGELOG.md`](CHANGELOG.md) に記録しています。この README の各言語版は英語版に従います。`README.md` を編集したら各 `README.<language>.md` とその元ハッシュを更新してください。`cargo xtask readme-sync` が一致を確認します。参加者は全員[行動規範](CODE_OF_CONDUCT.md)に従います。

## ライセンス

[Apache-2.0](LICENSE) © proob-ai
