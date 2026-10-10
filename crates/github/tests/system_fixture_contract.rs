//! Production GitHub client contract against the disposable system adapter.
//! This is fixture infrastructure coverage, not the controller issue-to-merge E2E.
use anyhow::{ensure, Context, Result};
use github::GithubRestClient;
use pocketflow_core::{CiStatus, MergeMethod, PrState};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Fixture {
    child: Child,
    root: tempfile::TempDir,
    url: String,
    scripts: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // SIGTERM lets the Python fixture stop its Git daemon and close logs.
        let _ = Command::new("kill")
            .args(["-TERM", &self.child.id().to_string()])
            .status();
        let _ = self.child.wait();
    }
}

impl Fixture {
    async fn start() -> Result<Self> {
        let root = tempfile::tempdir()?;
        let scripts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/system");
        let ready = root.path().join("ready.json");
        let log = std::fs::File::create(root.path().join("fixture.log"))?;
        let child = Command::new("python3")
            .arg(scripts.join("fixtures.py"))
            .args(["--service", "github", "--root"])
            .arg(root.path().join("service"))
            .arg("--ready-file")
            .arg(&ready)
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .spawn()?;
        let mut fixture = Self {
            child,
            root,
            url: String::new(),
            scripts,
        };
        let deadline = Instant::now() + Duration::from_secs(15);
        while !ready.exists() {
            ensure!(
                fixture.child.try_wait()?.is_none(),
                "fixture exited during startup"
            );
            ensure!(Instant::now() < deadline, "fixture startup timed out");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let metadata: Value = serde_json::from_slice(&std::fs::read(ready)?)?;
        fixture.url = metadata["url"]
            .as_str()
            .context("missing fixture URL")?
            .into();
        Ok(fixture)
    }

    fn git(&self, cwd: &Path, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "FORGE")
            .env("GIT_AUTHOR_EMAIL", "forge@example.test")
            .env("GIT_COMMITTER_NAME", "FORGE")
            .env("GIT_COMMITTER_EMAIL", "forge@example.test")
            .output()?;
        ensure!(
            output.status.success(),
            "Git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(String::from_utf8(output.stdout)?.trim().into())
    }

    fn ci(&self, remote: &str, sha: &str) -> Result<bool> {
        let output = Command::new("python3")
            .arg(self.scripts.join("run_ci.py"))
            .args([
                "--api", &self.url, "--remote", remote, "--sha", sha, "--oracle",
            ])
            .arg(self.scripts.join("acceptance.sh"))
            .arg("--artifacts")
            .arg(self.root.path().join("ci-artifacts"))
            .output()?;
        ensure!(
            matches!(output.status.code(), Some(0 | 1)),
            "CI runner failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(output.status.success())
    }
}

#[tokio::test]
async fn production_github_client_observes_real_ci_and_confirmed_git_merge() -> Result<()> {
    let fixture = Fixture::start().await?;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let forge = GithubRestClient::with_api_base("ci-forge-token", &fixture.url);
    let sentinel = GithubRestClient::with_api_base("ci-sentinel-token", &fixture.url);
    let vessel = GithubRestClient::with_api_base("ci-vessel-token", &fixture.url);
    ensure!(forge.get_authenticated_user_login().await? == "forge");
    http.post(format!("{}/repos/test/repo/issues", fixture.url))
        .bearer_auth("ci-operator-token")
        .json(&json!({"title": "Fix the answer", "body": "The required answer is 42."}))
        .send()
        .await?
        .error_for_status()?;
    let issues = forge.list_open_issues("test", "repo").await?;
    ensure!(issues.len() == 1 && issues[0].title == "Fix the answer");
    forge
        .comment_on_issue("test", "repo", issues[0].number, "Starting work")
        .await?;
    ensure!(
        forge
            .issue_has_comment_with_marker("test", "repo", issues[0].number, "Starting work")
            .await?
    );
    let repository: Value = http
        .get(format!("{}/repos/test/repo", fixture.url))
        .bearer_auth("ci-forge-token")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let remote = repository["clone_url"]
        .as_str()
        .context("missing Git remote")?;
    fixture.git(fixture.root.path(), &["clone", remote, "forge"])?;
    let checkout = fixture.root.path().join("forge");
    fixture.git(&checkout, &["switch", "-c", "fix/T-1"])?;
    fixture.git(
        &checkout,
        &["commit", "--allow-empty", "-m", "broken candidate"],
    )?;
    fixture.git(&checkout, &["push", "origin", "HEAD"])?;
    let broken = fixture.git(&checkout, &["rev-parse", "HEAD"])?;
    let pr = forge
        .create_pull_request(
            "test",
            "repo",
            "Fix T-1",
            "fix/T-1",
            "main",
            Some("Fixes #1"),
        )
        .await?;
    ensure!(vessel.get_pull_request("test", "repo", pr).await?.head_sha == broken);
    ensure!(vessel.get_ci_status("test", "repo", &broken).await? == CiStatus::Pending);
    ensure!(!fixture.ci(remote, &broken)?);
    ensure!(vessel.get_ci_status("test", "repo", &broken).await? == CiStatus::Failure);
    sentinel
        .submit_pull_request_review(
            "test",
            "repo",
            pr,
            "APPROVE",
            "Review",
            vec![],
            Some(&broken),
        )
        .await?;
    ensure!(
        !vessel
            .merge_pull_request("test", "repo", pr, "Merge", MergeMethod::Merge, &broken)
            .await?
            .merged
    );
    std::fs::write(checkout.join("answer.txt"), "42\n")?;
    fixture.git(&checkout, &["add", "answer.txt"])?;
    fixture.git(&checkout, &["commit", "-m", "fix answer"])?;
    fixture.git(&checkout, &["push", "origin", "HEAD"])?;
    let fixed = fixture.git(&checkout, &["rev-parse", "HEAD"])?;
    ensure!(fixture.ci(remote, &fixed)?);
    ensure!(vessel.get_ci_status("test", "repo", &fixed).await? == CiStatus::Success);
    ensure!(
        !vessel
            .merge_pull_request("test", "repo", pr, "Merge", MergeMethod::Merge, &broken)
            .await?
            .merged
    );
    ensure!(
        !vessel
            .merge_pull_request("test", "repo", pr, "Merge", MergeMethod::Merge, &fixed)
            .await?
            .merged
    );
    sentinel
        .submit_pull_request_review(
            "test",
            "repo",
            pr,
            "APPROVE",
            "Verified fixed head",
            vec![],
            Some(&fixed),
        )
        .await?;
    let reviews = vessel.list_pr_reviews("test", "repo", pr).await?;
    ensure!(reviews.len() == 2 && reviews[1].commit_id.as_deref() == Some(&fixed));
    let merged = vessel
        .merge_pull_request(
            "test",
            "repo",
            pr,
            "Merge verified fix",
            MergeMethod::Merge,
            &fixed,
        )
        .await?;
    ensure!(merged.merged);
    ensure!(vessel.confirmed_merge_sha("test", "repo", pr).await? == merged.sha);
    ensure!(vessel.get_pull_request("test", "repo", pr).await?.state == PrState::Merged);
    fixture.git(&checkout, &["fetch", "origin", "main"])?;
    ensure!(
        fixture.git(&checkout, &["rev-parse", "origin/main"])?
            == merged.sha.context("missing actual merge SHA")?
    );
    ensure!(fixture.git(&checkout, &["show", "origin/main:answer.txt"])? == "42");
    fixture.git(
        &checkout,
        &["merge-base", "--is-ancestor", &fixed, "origin/main"],
    )?;
    Ok(())
}
