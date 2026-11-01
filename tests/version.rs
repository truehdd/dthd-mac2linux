use dthd_mac2linux::version;
use std::{fs, path::Path, process::Command};

struct Repository(tempfile::TempDir);

impl Repository {
    fn new() -> Self {
        let repo = Self(tempfile::tempdir().unwrap());
        repo.git(&["init", "--quiet", "--initial-branch=main"]);
        repo.git(&["config", "user.name", "Version test"]);
        repo.git(&["config", "user.email", "test@example.invalid"]);
        fs::write(repo.path().join("source"), "original\n").unwrap();
        repo.git(&["add", "source"]);
        repo.git(&["commit", "--quiet", "-m", "initial"]);
        repo
    }

    fn path(&self) -> &Path {
        self.0.path()
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(self.path())
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", self.path().join("no-global-config"))
            .env("GIT_AUTHOR_DATE", "2001-02-03T23:45:00-0500")
            .env("GIT_COMMITTER_DATE", "2001-02-03T23:45:00-0500")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn tag(&self, name: &str) {
        self.git(&["tag", "-a", name, "-m", "release"]);
    }
}

#[test]
fn exact_tag_and_ahead_use_abbreviated_commit_ids() {
    let repo = Repository::new();
    repo.tag("v1.2.3-rc.1");
    let commit = repo.git(&["rev-parse", "--short=7", "HEAD"]);
    assert_eq!(commit.len(), 7);
    assert_eq!(
        version::describe(repo.path()).unwrap(),
        (format!("v1.2.3-rc.1-g{commit}"), "2001-02-03".into())
    );

    for _ in 0..2 {
        repo.git(&["commit", "--quiet", "--allow-empty", "-m", "next"]);
    }
    repo.git(&["tag", "v9.9.9"]);
    repo.tag("notes");
    let commit = repo.git(&["rev-parse", "--short=7", "HEAD"]);
    assert_eq!(
        version::describe(repo.path()).unwrap().0,
        format!("v1.2.3-rc.1+2-g{commit}")
    );
}

#[test]
fn tracked_changes_are_dirty_but_local_outputs_are_not() {
    let repo = Repository::new();
    repo.tag("v1.0.0");
    let clean = version::describe(repo.path()).unwrap();

    fs::write(repo.path().join("untracked"), "output").unwrap();
    assert_eq!(version::describe(repo.path()).unwrap(), clean);

    fs::write(repo.path().join("source"), "modified\n").unwrap();
    assert_eq!(
        version::describe(repo.path()).unwrap().0,
        format!("{}-dirty", clean.0)
    );
    repo.git(&["add", "source"]);
    assert_eq!(
        version::describe(repo.path()).unwrap().0,
        format!("{}-dirty", clean.0)
    );
}

#[test]
fn missing_and_invalid_release_tags_are_rejected() {
    let repo = Repository::new();
    assert!(version::describe(repo.path()).is_err());
    repo.git(&["tag", "v1.0.0"]);
    assert!(version::describe(repo.path()).is_err());
    repo.tag("v2.invalid");
    assert!(version::describe(repo.path()).is_err());

    let archive = tempfile::tempdir().unwrap();
    assert!(version::describe(archive.path()).is_err());
}

#[test]
fn linked_worktrees_use_their_own_head() {
    let repo = Repository::new();
    repo.tag("v1.0.0");
    let directory = tempfile::tempdir().unwrap();
    let worktree = directory.path().join("checkout");
    repo.git(&["worktree", "add", "--detach", worktree.to_str().unwrap()]);
    let before = version::describe(&worktree).unwrap();
    repo.git(&["commit", "--quiet", "--allow-empty", "-m", "next"]);
    assert_eq!(version::describe(&worktree).unwrap(), before);
    assert_ne!(version::describe(repo.path()).unwrap(), before);

    let nested = worktree.join("nested");
    fs::create_dir(&nested).unwrap();
    assert!(version::describe(&nested).is_err());
}
