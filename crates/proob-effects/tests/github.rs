//! The client's plumbing without a server: the token never prints, the
//! budget counts every attempt, repository ids parse from the forms GitHub
//! and git give, and an unreachable API is a transport error, not a panic.
use proob_effects::error::ApiError;
use proob_effects::github::{CallBudget, GitHubApi, GitHubClient, RepoId, Token};

#[test]
fn the_token_is_never_printed() {
    let token = Token::new("  ghp_secretvalue123  ").expect("non-empty");
    assert_eq!(format!("{token:?}"), "Token([redacted])");
    assert_eq!(token.to_string(), "[redacted]");
    assert!(!format!("{token:?}{token}").contains("secret"));
    assert!(Token::new("   ").is_none());
}

#[test]
fn the_budget_counts_down_and_refuses_when_spent() {
    let budget = CallBudget::new(2);
    assert!(budget.spend().is_ok());
    assert!(budget.spend().is_ok());
    assert_eq!(budget.spend(), Err(ApiError::RateLimited));
    assert_eq!((budget.remaining(), budget.spent()), (0, 2));
}

#[test]
fn repository_ids_parse() {
    let want = RepoId { owner: "proob-ai".into(), name: "proob".into() };
    assert_eq!(RepoId::parse("proob-ai/proob"), Some(want.clone()));
    assert_eq!(RepoId::parse("proob-ai/proob.git"), Some(want.clone()));
    assert_eq!(RepoId::parse("proob"), None);
    assert_eq!(RepoId::parse("a/b/c"), None);
    assert_eq!(RepoId::from_remote_url("https://github.com/proob-ai/proob.git"), Some(want.clone()));
    assert_eq!(RepoId::from_remote_url("git@github.com:proob-ai/proob.git"), Some(want.clone()));
    assert_eq!(RepoId::from_remote_url("ssh://git@github.com/proob-ai/proob"), Some(want));
    assert_eq!(RepoId::from_remote_url("nonsense"), None);
}

#[test]
fn an_unreachable_api_is_a_transport_error_and_spends_a_call() {
    let budget = CallBudget::new(3);
    let repo = RepoId { owner: "o".into(), name: "r".into() };
    // Port 1 refuses connections at once on every platform CI uses.
    let client = GitHubClient::with_base("http://127.0.0.1:1", repo, Token::new("x").expect("token"), &budget)
        .expect("a client builds offline");
    let err = client.pull(7).unwrap_err();
    assert!(matches!(err, ApiError::Transport { .. }), "{err:?}");
    assert!(err.to_string().contains("/repos/o/r/pulls/7"), "{err}");
    assert!(!err.to_string().contains("Bearer"), "no token in errors");
    assert_eq!(budget.spent(), 1);

    // A spent budget never reaches the network.
    budget.spend().expect("second");
    budget.spend().expect("third");
    assert_eq!(client.pull(7), Err(ApiError::RateLimited));
}
