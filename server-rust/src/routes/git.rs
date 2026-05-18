//! Git routes — mirrors server/routes/git.js
//!
//! All 20 git endpoints under /api/git:
//!   1. GET  /status              — full git status
//!   2. GET  /diff                — diff for a specific file
//!   3. GET  /file-with-diff      — file content with old/new for editor
//!   4. POST /initial-commit      — create initial commit
//!   5. POST /commit              — stage and commit files
//!   6. POST /revert-local-commit — soft reset most recent local commit
//!   7. GET  /branches            — list local and remote branches
//!   8. POST /checkout            — switch branch
//!   9. POST /create-branch       — create and switch to new branch
//!  10. POST /delete-branch       — delete local branch
//!  11. GET  /commits             — recent commits with stats
//!  12. GET  /commit-diff         — diff for a specific commit
//!  13. POST /generate-commit-message — AI commit message generation (stub)
//!  14. GET  /remote-status       — ahead/behind counts
//!  15. POST /fetch               — git fetch
//!  16. POST /pull                — git pull
//!  17. POST /push                — git push
//!  18. POST /publish             — publish branch to remote
//!  19. POST /discard             — discard file changes
//!  20. POST /delete-untracked    — delete untracked files

use axum::{
    extract::Query,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Extension, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

use crate::auth::middleware::AuthUser;
use crate::db::repos::projects::ProjectsRepo;

const COMMIT_DIFF_CHARACTER_LIMIT: usize = 500_000;

// ── Route Registration ─────────────────────────────────────────────────────

pub fn routes() -> Router {
    Router::new()
        .route("/status", get(git_status))
        .route("/diff", get(git_diff))
        .route("/file-with-diff", get(git_file_with_diff))
        .route("/initial-commit", post(git_initial_commit))
        .route("/commit", post(git_commit))
        .route("/revert-local-commit", post(git_revert_local_commit))
        .route("/branches", get(git_branches))
        .route("/checkout", post(git_checkout))
        .route("/create-branch", post(git_create_branch))
        .route("/delete-branch", post(git_delete_branch))
        .route("/commits", get(git_commits))
        .route("/commit-diff", get(git_commit_diff))
        .route("/generate-commit-message", post(git_generate_commit_message))
        .route("/remote-status", get(git_remote_status))
        .route("/fetch", post(git_fetch))
        .route("/pull", post(git_pull))
        .route("/push", post(git_push))
        .route("/publish", post(git_publish))
        .route("/discard", post(git_discard))
        .route("/delete-untracked", post(git_delete_untracked))
}

// ── Request / Query Types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ProjectQuery {
    project: String,
}

#[derive(Debug, Deserialize)]
struct DiffQuery {
    project: String,
    file: String,
}

#[derive(Debug, Deserialize)]
struct CommitsQuery {
    project: String,
    limit: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CommitDiffQuery {
    project: String,
    commit: String,
}

#[derive(Debug, Deserialize)]
struct CommitBody {
    project: String,
    message: String,
    files: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BranchBody {
    project: String,
    branch: String,
}

#[derive(Debug, Deserialize)]
struct FileBody {
    project: String,
    file: String,
}

#[derive(Debug, Deserialize)]
struct GenerateMessageBody {
    project: String,
    files: Vec<String>,
    #[serde(rename = "provider")]
    provider: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectBody {
    project: String,
}

// ── Core Git Helpers ───────────────────────────────────────────────────────

/// Run a git command in the given working directory, returning (stdout, stderr).
async fn run_git(args: &[&str], cwd: &str) -> Result<(String, String), String> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| format!("Failed to execute git: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok((stdout.trim().to_string(), stderr.trim().to_string()))
    } else {
        let combined = format!("{} {}", stderr.trim(), stdout.trim());
        let msg = combined.trim();
        Err(if msg.is_empty() {
            "Git command failed".to_string()
        } else {
            msg.to_string()
        })
    }
}

/// Resolve the absolute project directory for a given DB project_id.
async fn get_project_path(project_id: &str) -> Result<String, (StatusCode, Json<Value>)> {
    ProjectsRepo::get_project_path_by_id(project_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("Unable to resolve project path for \"{}\"", project_id)})),
        )
    })
}

/// Validate that a path is inside a git repository.
async fn validate_git_repository(project_path: &str) -> Result<(), (StatusCode, Json<Value>)> {
    // Check directory exists
    tokio::fs::metadata(project_path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": format!("Project path not found: {}", project_path)})),
            )
        } else {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        }
    })?;

    // Check inside work tree
    let (stdout, _) =
        run_git(&["rev-parse", "--is-inside-work-tree"], project_path).await.map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Not a git repository. This directory does not contain a .git folder. Initialize a git repository with \"git init\" to use source control features."})),
            )
        })?;

    if stdout.trim() != "true" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Not inside a git work tree"})),
        ));
    }

    // Ensure git root is resolvable
    run_git(&["rev-parse", "--show-toplevel"], project_path).await.map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Not a git repository."})),
        )
    })?;

    Ok(())
}

/// Get the current branch name.
async fn get_current_branch_name(project_path: &str) -> String {
    // Try symbolic-ref first (works even without commits)
    if let Ok((stdout, _)) = run_git(&["symbolic-ref", "--short", "HEAD"], project_path).await {
        let branch = stdout.trim().to_string();
        if !branch.is_empty() {
            return branch;
        }
    }
    // Fallback to rev-parse (detached HEAD or older git)
    if let Ok((stdout, _)) = run_git(&["rev-parse", "--abbrev-ref", "HEAD"], project_path).await {
        let branch = stdout.trim().to_string();
        if !branch.is_empty() {
            return branch;
        }
    }
    "unknown".to_string()
}

/// Check whether the repository has any commits.
async fn repository_has_commits(project_path: &str) -> bool {
    match run_git(&["rev-parse", "--verify", "HEAD"], project_path).await {
        Ok(_) => true,
        Err(e) => {
            let lower = e.to_lowercase();
            !(lower.contains("unknown revision")
                || lower.contains("ambiguous argument")
                || lower.contains("needed a single revision")
                || lower.contains("bad revision"))
        }
    }
}

/// Get the repository root path (via git rev-parse --show-toplevel).
async fn get_repository_root_path(
    project_path: &str,
) -> Result<String, (StatusCode, Json<Value>)> {
    let (stdout, _) = run_git(&["rev-parse", "--show-toplevel"], project_path).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
    })?;
    Ok(stdout.trim().to_string())
}

// ── Validation Helpers ─────────────────────────────────────────────────────

fn validate_branch_name(branch: &str) -> Result<(), String> {
    if branch.is_empty()
        || !branch
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '/' || c == '-')
    {
        return Err("Invalid branch name".to_string());
    }
    Ok(())
}

fn validate_commit_ref(commit: &str) -> Result<(), String> {
    if commit.is_empty()
        || !commit
            .chars()
            .all(|c| c.is_alphanumeric() || "._~{}@/-".contains(c))
    {
        return Err("Invalid commit reference".to_string());
    }
    Ok(())
}

fn validate_file_path(file_path: &str) -> Result<(), String> {
    if file_path.is_empty() || file_path.contains('\0') {
        return Err("Invalid file path".to_string());
    }
    Ok(())
}

fn validate_remote_name(remote: &str) -> Result<(), String> {
    if remote.is_empty()
        || !remote
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '-')
    {
        return Err("Invalid remote name".to_string());
    }
    Ok(())
}

// ── Path Helpers ───────────────────────────────────────────────────────────

/// Normalize a repository-relative file path: backslash to forward slash,
/// strip leading `./` and `/`, trim whitespace.
fn normalize_repository_relative_file_path(file_path: &str) -> String {
    let s = file_path.replace('\\', "/");
    let s = s.trim_start_matches("./");
    let s = s.trim_start_matches('/');
    s.trim().to_string()
}

struct ResolvedFile {
    repo_root: String,
    relative_path: String,
}

/// Build candidate repo-relative paths for a given file path.
fn build_file_path_candidates(project_path: &str, repo_root: &str, file_path: &str) -> Vec<String> {
    let normalized = normalize_repository_relative_file_path(file_path);
    let project_relative = {
        let pp = Path::new(project_path);
        let rr = Path::new(repo_root);
        if let Ok(rel) = pp.strip_prefix(rr) {
            normalize_repository_relative_file_path(&rel.to_string_lossy())
        } else {
            String::new()
        }
    };

    let mut candidates = vec![normalized.clone()];

    if !project_relative.is_empty()
        && project_relative != "."
        && !normalized.starts_with(&format!("{}/", project_relative))
    {
        candidates.push(format!("{}/{}", project_relative, normalized));
    }

    candidates.sort();
    candidates.dedup();
    candidates.into_iter().filter(|c| !c.is_empty()).collect()
}

/// Parse git status --porcelain output to extract file paths.
fn parse_status_file_paths(output: &str) -> Vec<String> {
    output
        .lines()
        .map(|l| l.trim_end())
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let status_path = if l.len() > 3 { &l[3..] } else { "" };
            let renamed = status_path.split(" -> ").nth(1);
            normalize_repository_relative_file_path(renamed.unwrap_or(status_path))
        })
        .filter(|p| !p.is_empty())
        .collect()
}

/// Resolve a caller-supplied file path to a repo-relative path.
async fn resolve_repository_file_path(
    project_path: &str,
    file_path: &str,
) -> Result<ResolvedFile, (StatusCode, Json<Value>)> {
    validate_file_path(file_path).map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({"error": e}))))?;

    let repo_root = get_repository_root_path(project_path).await?;
    let candidates = build_file_path_candidates(project_path, &repo_root, file_path);

    for candidate in &candidates {
        if let Ok((stdout, _)) =
            run_git(&["status", "--porcelain", "--", candidate], &repo_root).await
        {
            if !stdout.trim().is_empty() {
                return Ok(ResolvedFile {
                    repo_root,
                    relative_path: candidate.clone(),
                });
            }
        }
    }

    // Bare filename: search all changed files for a suffix match
    let normalized = normalize_repository_relative_file_path(file_path);
    if !normalized.contains('/') {
        if let Ok((stdout, _)) = run_git(&["status", "--porcelain"], &repo_root).await {
            let changed_paths = parse_status_file_paths(&stdout);
            for cp in &changed_paths {
                if cp == &normalized || cp.ends_with(&format!("/{}", normalized)) {
                    return Ok(ResolvedFile {
                        repo_root,
                        relative_path: cp.clone(),
                    });
                }
            }
        }
    }

    Ok(ResolvedFile {
        repo_root,
        relative_path: candidates.into_iter().next().unwrap_or_default(),
    })
}

// ── Diff / Content Helpers ─────────────────────────────────────────────────

/// Strip git diff header lines, keeping only hunk content.
fn strip_diff_headers(diff: &str) -> String {
    if diff.is_empty() {
        return String::new();
    }

    let mut result: Vec<&str> = Vec::new();
    let mut start_including = false;

    for line in diff.lines() {
        if line.starts_with("diff --git")
            || line.starts_with("index ")
            || line.starts_with("new file mode")
            || line.starts_with("deleted file mode")
            || line.starts_with("---")
            || line.starts_with("+++")
        {
            continue;
        }
        if line.starts_with("@@") || start_including {
            start_including = true;
            result.push(line);
        }
    }

    result.join("\n")
}

/// Get the old (HEAD) content for a tracked file in the repository.
async fn get_head_file_content(
    repo_root: &str,
    relative_path: &str,
) -> Option<String> {
    match run_git(&["show", &format!("HEAD:{}", relative_path)], repo_root).await {
        Ok((stdout, _)) => Some(stdout),
        Err(_) => None,
    }
}

// ===========================================================================
// ENDPOINT HANDLERS — 1 through 20
// ===========================================================================

// ── 1. GET /status ─────────────────────────────────────────────────────────
/// Return full git status: branch, hasCommits, and categorised file lists.

async fn git_status(
    _user: Extension<AuthUser>,
    Query(query): Query<ProjectQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = get_project_path(&query.project).await?;
    validate_git_repository(&project_path).await?;

    let branch = get_current_branch_name(&project_path).await;
    let has_commits = repository_has_commits(&project_path).await;

    let (status_stdout, _) =
        run_git(&["status", "--porcelain"], &project_path).await.map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
        })?;

    let mut modified: Vec<String> = Vec::new();
    let mut added: Vec<String> = Vec::new();
    let mut deleted: Vec<String> = Vec::new();
    let mut untracked: Vec<String> = Vec::new();

    for line in status_stdout.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        let status = if trimmed.len() >= 2 { &trimmed[..2] } else { continue };
        let file = if trimmed.len() > 3 { &trimmed[3..] } else { continue };

        match status {
            "M " | " M" | "MM" => modified.push(file.to_string()),
            "A " | "AM" => added.push(file.to_string()),
            "D " | " D" => deleted.push(file.to_string()),
            "??" => untracked.push(file.to_string()),
            _ => {}
        }
    }

    Ok(Json(json!({
        "branch": branch,
        "hasCommits": has_commits,
        "modified": modified,
        "added": added,
        "deleted": deleted,
        "untracked": untracked
    })))
}

// ── 2. GET /diff ───────────────────────────────────────────────────────────
/// Return the diff for a specific file (untracked/deleted/modified/staged).

async fn git_diff(
    _user: Extension<AuthUser>,
    Query(query): Query<DiffQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = get_project_path(&query.project).await?;
    validate_git_repository(&project_path).await?;

    let resolved = resolve_repository_file_path(&project_path, &query.file).await?;

    // Check file status
    let (status_stdout, _) = run_git(
        &["status", "--porcelain", "--", &resolved.relative_path],
        &resolved.repo_root,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))))?;

    let is_untracked = status_stdout.starts_with("??");
    let trimmed_status = status_stdout.trim();
    let is_deleted = trimmed_status.starts_with("D ") || trimmed_status.starts_with(" D");

    let diff = if is_untracked {
        // Untracked file: show entire content as additions
        let file_path = Path::new(&resolved.repo_root).join(&resolved.relative_path);
        match tokio::fs::metadata(&file_path).await {
            Ok(meta) if meta.is_dir() => {
                format!("Directory: {}\n(Cannot show diff for directories)", resolved.relative_path)
            }
            Ok(_) => {
                match tokio::fs::read_to_string(&file_path).await {
                    Ok(content) => {
                        let lines: Vec<&str> = content.lines().collect();
                        let mut d = format!(
                            "--- /dev/null\n+++ b/{}\n@@ -0,0 +1,{} @@\n",
                            resolved.relative_path,
                            lines.len()
                        );
                        for line in &lines {
                            d.push_str(&format!("+{}\n", line));
                        }
                        d
                    }
                    Err(e) => return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": e.to_string()})),
                    )),
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )),
        }
    } else if is_deleted {
        // Deleted file: show entire content as deletions from HEAD
        match get_head_file_content(&resolved.repo_root, &resolved.relative_path).await {
            Some(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let mut d = format!(
                    "--- a/{}\n+++ /dev/null\n@@ -1,{} +0,0 @@\n",
                    resolved.relative_path,
                    lines.len()
                );
                for line in &lines {
                    d.push_str(&format!("-{}\n", line));
                }
                d
            }
            None => String::new(),
        }
    } else {
        // Tracked file: unstaged diff first, then staged
        if let Ok((stout, _)) =
            run_git(&["diff", "--", &resolved.relative_path], &resolved.repo_root).await
        {
            if !stout.is_empty() {
                strip_diff_headers(&stout)
            } else if let Ok((cached_stdout, _)) = run_git(
                &["diff", "--cached", "--", &resolved.relative_path],
                &resolved.repo_root,
            )
            .await
            {
                if !cached_stdout.is_empty() {
                    strip_diff_headers(&cached_stdout)
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    };

    Ok(Json(json!({ "diff": diff })))
}

// ── 3. GET /file-with-diff ─────────────────────────────────────────────────
/// Return file content with old/new versions for the CodeEditor.

async fn git_file_with_diff(
    _user: Extension<AuthUser>,
    Query(query): Query<DiffQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = get_project_path(&query.project).await?;
    validate_git_repository(&project_path).await?;

    let resolved = resolve_repository_file_path(&project_path, &query.file).await?;

    // Check file status
    let (status_stdout, _) = run_git(
        &["status", "--porcelain", "--", &resolved.relative_path],
        &resolved.repo_root,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))))?;

    let is_untracked = status_stdout.starts_with("??");
    let trimmed_status = status_stdout.trim();
    let is_deleted = trimmed_status.starts_with("D ") || trimmed_status.starts_with(" D");

    let file_path = Path::new(&resolved.repo_root).join(&resolved.relative_path);

    // Prevent diff on directories
    if !is_deleted {
        if let Ok(meta) = tokio::fs::metadata(&file_path).await {
            if meta.is_dir() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "Cannot show diff for directories"})),
                ));
            }
        }
    }

    let (current_content, old_content) = if is_deleted {
        // Deleted: get content from HEAD, use as both old and current
        let head_content = get_head_file_content(&resolved.repo_root, &resolved.relative_path)
            .await
            .unwrap_or_default();
        (head_content.clone(), head_content)
    } else {
        // Read current content from filesystem
        let current = tokio::fs::read_to_string(&file_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                (StatusCode::NOT_FOUND, Json(json!({"error": "File not found"})))
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))
            }
        })?;

        let old = if is_untracked {
            String::new()
        } else {
            get_head_file_content(&resolved.repo_root, &resolved.relative_path)
                .await
                .unwrap_or_default()
        };

        (current, old)
    };

    Ok(Json(json!({
        "currentContent": current_content,
        "oldContent": old_content,
        "isDeleted": is_deleted,
        "isUntracked": is_untracked
    })))
}

// ── 4. POST /initial-commit ────────────────────────────────────────────────
/// Create the very first commit in a fresh repository (git add . && git commit).

async fn git_initial_commit(
    _user: Extension<AuthUser>,
    Json(body): Json<ProjectBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = get_project_path(&body.project).await?;
    validate_git_repository(&project_path).await?;

    // Refuse if HEAD already exists
    if repository_has_commits(&project_path).await {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Repository already has commits. Use regular commit instead."})),
        ));
    }

    // Add all files
    run_git(&["add", "."], &project_path)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to stage files"})),
            )
        })?;

    // Create initial commit
    match run_git(&["commit", "-m", "Initial commit"], &project_path).await {
        Ok((stdout, _)) => Ok(Json(json!({
            "success": true,
            "output": stdout,
            "message": "Initial commit created successfully"
        }))),
        Err(e) => {
            if e.to_lowercase().contains("nothing to commit") {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "Nothing to commit",
                        "details": "No files found in the repository. Add some files first."
                    })),
                ));
            }
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e})),
            ))
        }
    }
}

// ── 5. POST /commit ────────────────────────────────────────────────────────
/// Stage selected files and create a commit with the given message.

async fn git_commit(
    _user: Extension<AuthUser>,
    Json(body): Json<CommitBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if body.message.is_empty() || body.files.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Project name, commit message, and files are required"})),
        ));
    }

    let project_path = get_project_path(&body.project).await?;
    validate_git_repository(&project_path).await?;
    let repo_root = get_repository_root_path(&project_path).await?;

    // Stage each file
    for file in &body.files {
        let resolved = resolve_repository_file_path(&project_path, file).await?;
        run_git(&["add", "--", &resolved.relative_path], &resolved.repo_root).await.map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("Failed to stage {}: {}", file, e)})))
        })?;
    }

    // Commit
    let (stdout, _) = run_git(&["commit", "-m", &body.message], &repo_root).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
    })?;

    Ok(Json(json!({
        "success": true,
        "output": stdout
    })))
}

// ── 6. POST /revert-local-commit ───────────────────────────────────────────
/// Soft-reset the most recent local commit, keeping changes staged.

async fn git_revert_local_commit(
    _user: Extension<AuthUser>,
    Json(body): Json<ProjectBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = get_project_path(&body.project).await?;
    validate_git_repository(&project_path).await?;

    // Check there is a commit to revert
    if !repository_has_commits(&project_path).await {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "No local commit to revert",
                "details": "This repository has no commit yet."
            })),
        ));
    }

    match run_git(&["reset", "--soft", "HEAD~1"], &project_path).await {
        Ok(_) => Ok(Json(json!({
            "success": true,
            "output": "Latest local commit reverted successfully. Changes were kept staged."
        }))),
        Err(e) => {
            // If HEAD~1 fails (initial commit), delete HEAD ref instead
            let lower = e.to_lowercase();
            if (lower.contains("unknown revision") || lower.contains("ambiguous argument"))
                && lower.contains("head~1")
            {
                run_git(&["update-ref", "-d", "HEAD"], &project_path).await.map_err(|e| {
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
                })?;

                Ok(Json(json!({
                    "success": true,
                    "output": "Latest local commit reverted successfully. Changes were kept staged."
                })))
            } else {
                Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": e})),
                ))
            }
        }
    }
}

// ── 7. GET /branches ───────────────────────────────────────────────────────
/// List local and remote branches.

async fn git_branches(
    _user: Extension<AuthUser>,
    Query(query): Query<ProjectQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = get_project_path(&query.project).await?;
    validate_git_repository(&project_path).await?;

    let (stdout, _) = run_git(&["branch", "-a"], &project_path).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
    })?;

    let raw_lines: Vec<&str> = stdout
        .lines()
        .map(|b| b.trim())
        .filter(|b| !b.is_empty() && !b.contains("->"))
        .collect();

    let local_branches: Vec<String> = raw_lines
        .iter()
        .filter(|b| !b.starts_with("remotes/"))
        .map(|b| {
            if b.starts_with("* ") {
                b[2..].to_string()
            } else {
                b.to_string()
            }
        })
        .collect();

    let remote_branches: Vec<String> = raw_lines
        .iter()
        .filter(|b| b.starts_with("remotes/"))
        .map(|b| {
            let without_remote = b
                .splitn(2, '/')
                .nth(1)
                .and_then(|s| s.splitn(2, '/').nth(1))
                .unwrap_or("");
            without_remote.to_string()
        })
        .filter(|name| !local_branches.contains(name))
        .collect();

    let all_branches: Vec<String> = local_branches
        .iter()
        .chain(remote_branches.iter())
        .map(|b| b.to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    Ok(Json(json!({
        "branches": all_branches,
        "localBranches": local_branches,
        "remoteBranches": remote_branches
    })))
}

// ── 8. POST /checkout ──────────────────────────────────────────────────────
/// Switch to an existing branch.

async fn git_checkout(
    _user: Extension<AuthUser>,
    Json(body): Json<BranchBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    validate_branch_name(&body.branch).map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": e})))
    })?;

    let project_path = get_project_path(&body.project).await?;
    validate_git_repository(&project_path).await?;

    let (stdout, _) = run_git(&["checkout", &body.branch], &project_path).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
    })?;

    Ok(Json(json!({
        "success": true,
        "output": stdout
    })))
}

// ── 9. POST /create-branch ─────────────────────────────────────────────────
/// Create a new branch and switch to it.

async fn git_create_branch(
    _user: Extension<AuthUser>,
    Json(body): Json<BranchBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    validate_branch_name(&body.branch).map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": e})))
    })?;

    let project_path = get_project_path(&body.project).await?;
    validate_git_repository(&project_path).await?;

    let (stdout, _) = run_git(&["checkout", "-b", &body.branch], &project_path).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
    })?;

    Ok(Json(json!({
        "success": true,
        "output": stdout
    })))
}

// ── 10. POST /delete-branch ────────────────────────────────────────────────
/// Delete a local branch (safety: prevents deleting the current branch).

async fn git_delete_branch(
    _user: Extension<AuthUser>,
    Json(body): Json<BranchBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    validate_branch_name(&body.branch).map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": e})))
    })?;

    let project_path = get_project_path(&body.project).await?;
    validate_git_repository(&project_path).await?;

    // Safety: cannot delete the currently checked-out branch
    let current = get_current_branch_name(&project_path).await;
    if current == body.branch {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Cannot delete the currently checked-out branch"})),
        ));
    }

    let (stdout, _) = run_git(&["branch", "-d", &body.branch], &project_path).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
    })?;

    Ok(Json(json!({
        "success": true,
        "output": stdout
    })))
}

// ── 11. GET /commits ───────────────────────────────────────────────────────
/// Return recent commits with per-commit stats.

async fn git_commits(
    _user: Extension<AuthUser>,
    Query(query): Query<CommitsQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = get_project_path(&query.project).await?;
    validate_git_repository(&project_path).await?;

    let parsed_limit: usize = query
        .limit
        .as_deref()
        .and_then(|l| l.parse().ok())
        .filter(|n| *n > 0)
        .map(|n: usize| n.min(100))
        .unwrap_or(10);

    let (stdout, _) = run_git(
        &[
            "log",
            "--pretty=format:%H|%an|%ae|%ad|%s",
            "--date=iso-strict",
            "-n",
            &parsed_limit.to_string(),
        ],
        &project_path,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))))?;

    let mut commits: Vec<Value> = Vec::new();

    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parts: Vec<&str> = trimmed.splitn(5, '|').collect();
        if parts.len() < 5 {
            continue;
        }
        let hash = parts[0].to_string();
        let author = parts[1].to_string();
        let email = parts[2].to_string();
        let date = parts[3].to_string();
        let message = parts[4].to_string();

        // Get stats for this commit
        let stats = match run_git(&["show", "--stat", "--format=", &hash], &project_path).await {
            Ok((sout, _)) => sout.lines().last().unwrap_or("").trim().to_string(),
            Err(_) => String::new(),
        };

        commits.push(json!({
            "hash": hash,
            "author": author,
            "email": email,
            "date": date,
            "message": message,
            "stats": stats
        }));
    }

    Ok(Json(json!({ "commits": commits })))
}

// ── 12. GET /commit-diff ───────────────────────────────────────────────────
/// Return the full diff for a specific commit.

async fn git_commit_diff(
    _user: Extension<AuthUser>,
    Query(query): Query<CommitDiffQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    validate_commit_ref(&query.commit).map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": e})))
    })?;

    let project_path = get_project_path(&query.project).await?;
    validate_git_repository(&project_path).await?;

    let (stdout, _) = run_git(&["show", &query.commit], &project_path).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
    })?;

    let is_truncated = stdout.len() > COMMIT_DIFF_CHARACTER_LIMIT;
    let diff = if is_truncated {
        format!(
            "{}\n\n... Diff truncated to keep the UI responsive ...",
            &stdout[..COMMIT_DIFF_CHARACTER_LIMIT]
        )
    } else {
        stdout
    };

    Ok(Json(json!({
        "diff": diff,
        "isTruncated": is_truncated
    })))
}

// ── 13. POST /generate-commit-message ──────────────────────────────────────
/// Generate a commit message based on diffs (AI stub).

async fn git_generate_commit_message(
    _user: Extension<AuthUser>,
    Json(body): Json<GenerateMessageBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if body.files.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Project id and files are required"})),
        ));
    }

    let project_path = get_project_path(&body.project).await?;
    validate_git_repository(&project_path).await?;
    let repo_root = get_repository_root_path(&project_path).await?;

    // Collect diff context for all files
    let mut diff_context = String::new();

    for file in &body.files {
        match resolve_repository_file_path(&project_path, file).await {
            Ok(resolved) => {
                if let Ok((stdout, _)) =
                    run_git(&["diff", "HEAD", "--", &resolved.relative_path], &resolved.repo_root).await
                {
                    if !stdout.is_empty() {
                        diff_context.push_str(&format!("\n--- {} ---\n{}", resolved.relative_path, stdout));
                    }
                }
            }
            Err(_) => {
                // Try reading as untracked file
                let file_path = Path::new(&repo_root).join(file);
                if let Ok(content) = tokio::fs::read_to_string(&file_path).await {
                    let truncated = content.chars().take(1000).collect::<String>();
                    diff_context.push_str(&format!("\n--- {} (new file) ---\n{}", file, truncated));
                }
            }
        }
    }

    // Stub: generate a simple conventional commit message based on file extensions
    let message = if diff_context.trim().is_empty() {
        format!("chore: update {} file(s)", body.files.len())
    } else {
        let has_feat = body.files.iter().any(|f| {
            f.ends_with(".rs")
                || f.ends_with(".ts")
                || f.ends_with(".tsx")
                || f.ends_with(".js")
                || f.ends_with(".jsx")
        });
        let has_docs = body.files.iter().any(|f| f.ends_with(".md") || f.ends_with(".mdx"));

        let type_str = if has_feat {
            "feat"
        } else if has_docs {
            "docs"
        } else {
            "chore"
        };

        format!("{}: update {} file(s)", type_str, body.files.len())
    };

    Ok(Json(json!({ "message": message })))
}

// ── 14. GET /remote-status ─────────────────────────────────────────────────
/// Return ahead/behind counts for the current branch vs its upstream.

async fn git_remote_status(
    _user: Extension<AuthUser>,
    Query(query): Query<ProjectQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = get_project_path(&query.project).await?;
    validate_git_repository(&project_path).await?;

    let branch = get_current_branch_name(&project_path).await;
    let has_commits = repository_has_commits(&project_path).await;

    // Check what remotes exist
    let (remote_stdout, _) = run_git(&["remote"], &project_path).await.unwrap_or_default();
    let remotes: Vec<&str> = remote_stdout.lines().filter(|r| !r.trim().is_empty()).collect();
    let has_remote = !remotes.is_empty();
    let fallback_remote = if remotes.contains(&"origin") {
        "origin"
    } else {
        remotes.first().copied().unwrap_or("origin")
    };

    // No commits — return early state
    if !has_commits {
        return Ok(Json(json!({
            "hasRemote": has_remote,
            "hasUpstream": false,
            "branch": branch,
            "remoteName": fallback_remote,
            "ahead": 0,
            "behind": 0,
            "isUpToDate": false,
            "message": "Repository has no commits yet"
        })));
    }

    // Check for upstream tracking branch
    let upstream_ref = format!("{}@{{upstream}}", branch);
    let tracking_branch = match run_git(&["rev-parse", "--abbrev-ref", &upstream_ref], &project_path).await {
        Ok((stdout, _)) => stdout.trim().to_string(),
        Err(_) => {
            return Ok(Json(json!({
                "hasRemote": has_remote,
                "hasUpstream": false,
                "branch": branch,
                "remoteName": fallback_remote,
                "message": "No remote tracking branch configured"
            })));
        }
    };

    if tracking_branch.is_empty() {
        return Ok(Json(json!({
            "hasRemote": has_remote,
            "hasUpstream": false,
            "branch": branch,
            "remoteName": fallback_remote,
            "message": "No remote tracking branch configured"
        })));
    }

    let remote_name = tracking_branch.split('/').next().unwrap_or("origin").to_string();

    // Get ahead/behind counts
    let (count_stdout, _) = run_git(
        &["rev-list", "--count", "--left-right", &format!("{}...HEAD", tracking_branch)],
        &project_path,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))))?;

    let parts: Vec<&str> = count_stdout.split('\t').collect();
    let behind: i64 = parts.first().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    let ahead: i64 = parts.get(1).and_then(|s| s.trim().parse().ok()).unwrap_or(0);

    Ok(Json(json!({
        "hasRemote": true,
        "hasUpstream": true,
        "branch": branch,
        "remoteBranch": tracking_branch,
        "remoteName": remote_name,
        "ahead": ahead,
        "behind": behind,
        "isUpToDate": ahead == 0 && behind == 0
    })))
}

// ── 15. POST /fetch ────────────────────────────────────────────────────────
/// Fetch from the upstream remote.

async fn git_fetch(
    _user: Extension<AuthUser>,
    Json(body): Json<ProjectBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = get_project_path(&body.project).await?;
    validate_git_repository(&project_path).await?;

    let branch = get_current_branch_name(&project_path).await;

    // Detect remote from upstream tracking, fallback to origin
    let upstream_ref = format!("{}@{{upstream}}", branch);
    let remote_name = match run_git(&["rev-parse", "--abbrev-ref", &upstream_ref], &project_path).await {
        Ok((stdout, _)) => stdout.trim().split('/').next().unwrap_or("origin").to_string(),
        Err(_) => "origin".to_string(),
    };

    validate_remote_name(&remote_name).map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": e})))
    })?;

    match run_git(&["fetch", &remote_name], &project_path).await {
        Ok((stdout, _)) => Ok(Json(json!({
            "success": true,
            "output": if stdout.is_empty() { "Fetch completed successfully" } else { &stdout },
            "remoteName": remote_name
        }))),
        Err(e) => {
            let (error, details) = classify_remote_error(&e, "Fetch failed");
            Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": error, "details": details }))))
        }
    }
}

// ── 16. POST /pull ─────────────────────────────────────────────────────────
/// Pull (fetch + merge) from the upstream remote.

async fn git_pull(
    _user: Extension<AuthUser>,
    Json(body): Json<ProjectBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = get_project_path(&body.project).await?;
    validate_git_repository(&project_path).await?;

    let branch = get_current_branch_name(&project_path).await;

    // Detect remote and branch from upstream tracking
    let upstream_ref = format!("{}@{{upstream}}", branch);
    let (remote_name, remote_branch) = match run_git(&["rev-parse", "--abbrev-ref", &upstream_ref], &project_path).await {
        Ok((stdout, _)) => {
            let tracking = stdout.trim();
            let rn = tracking.split('/').next().unwrap_or("origin").to_string();
            let rb = tracking.splitn(2, '/').nth(1).unwrap_or(&branch).to_string();
            (rn, rb)
        }
        Err(_) => ("origin".to_string(), branch.clone()),
    };

    validate_remote_name(&remote_name).map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": e})))
    })?;
    validate_branch_name(&remote_branch).map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": e})))
    })?;

    match run_git(&["pull", &remote_name, &remote_branch], &project_path).await {
        Ok((stdout, _)) => Ok(Json(json!({
            "success": true,
            "output": if stdout.is_empty() { "Pull completed successfully" } else { &stdout },
            "remoteName": remote_name,
            "remoteBranch": remote_branch
        }))),
        Err(e) => {
            let (error, details) = classify_pull_error(&e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": error, "details": details }))))
        }
    }
}

// ── 17. POST /push ─────────────────────────────────────────────────────────
/// Push commits to the upstream remote.

async fn git_push(
    _user: Extension<AuthUser>,
    Json(body): Json<ProjectBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = get_project_path(&body.project).await?;
    validate_git_repository(&project_path).await?;

    let branch = get_current_branch_name(&project_path).await;

    // Detect remote and branch from upstream tracking
    let upstream_ref = format!("{}@{{upstream}}", branch);
    let (remote_name, remote_branch) = match run_git(&["rev-parse", "--abbrev-ref", &upstream_ref], &project_path).await {
        Ok((stdout, _)) => {
            let tracking = stdout.trim();
            let rn = tracking.split('/').next().unwrap_or("origin").to_string();
            let rb = tracking.splitn(2, '/').nth(1).unwrap_or(&branch).to_string();
            (rn, rb)
        }
        Err(_) => ("origin".to_string(), branch.clone()),
    };

    validate_remote_name(&remote_name).map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": e})))
    })?;
    validate_branch_name(&remote_branch).map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": e})))
    })?;

    match run_git(&["push", &remote_name, &remote_branch], &project_path).await {
        Ok((stdout, _)) => Ok(Json(json!({
            "success": true,
            "output": if stdout.is_empty() { "Push completed successfully" } else { &stdout },
            "remoteName": remote_name,
            "remoteBranch": remote_branch
        }))),
        Err(e) => {
            let (error, details) = classify_push_error(&e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": error, "details": details }))))
        }
    }
}

// ── 18. POST /publish ──────────────────────────────────────────────────────
/// Publish the current branch to a remote (set upstream and push).

async fn git_publish(
    _user: Extension<AuthUser>,
    Json(body): Json<BranchBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    validate_branch_name(&body.branch).map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": e})))
    })?;

    let project_path = get_project_path(&body.project).await?;
    validate_git_repository(&project_path).await?;

    // Verify branch matches current
    let current_branch = get_current_branch_name(&project_path).await;
    if current_branch != body.branch {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!(
                    "Branch mismatch. Current branch is {}, but trying to publish {}",
                    current_branch, body.branch
                )
            })),
        ));
    }

    // Check that a remote exists
    let (remote_stdout, _) = run_git(&["remote"], &project_path).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))
    })?;

    let remotes: Vec<&str> = remote_stdout.lines().filter(|r| !r.trim().is_empty()).collect();
    if remotes.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "No remote repository configured. Add a remote with: git remote add origin <url>"
            })),
        ));
    }

    let remote_name = if remotes.contains(&"origin") {
        "origin"
    } else {
        remotes[0]
    };

    validate_remote_name(remote_name).map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": e})))
    })?;

    match run_git(&["push", "--set-upstream", remote_name, &body.branch], &project_path).await {
        Ok((stdout, _)) => Ok(Json(json!({
            "success": true,
            "output": if stdout.is_empty() { "Branch published successfully" } else { &stdout },
            "remoteName": remote_name,
            "branch": body.branch
        }))),
        Err(e) => {
            let (error, details) = classify_publish_error(&e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": error, "details": details }))))
        }
    }
}

// ── 19. POST /discard ──────────────────────────────────────────────────────
/// Discard changes for a specific file (restore/unstage/delete).

async fn git_discard(
    _user: Extension<AuthUser>,
    Json(body): Json<FileBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = get_project_path(&body.project).await?;
    validate_git_repository(&project_path).await?;

    let resolved = resolve_repository_file_path(&project_path, &body.file).await?;

    // Check file status
    let (status_stdout, _) = run_git(
        &["status", "--porcelain", "--", &resolved.relative_path],
        &resolved.repo_root,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))))?;

    if status_stdout.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "No changes to discard for this file"})),
        ));
    }

    let status = if status_stdout.len() >= 2 {
        &status_stdout[..2]
    } else {
        ""
    };

    match status {
        "??" => {
            // Untracked: delete from filesystem
            let full_path = Path::new(&resolved.repo_root).join(&resolved.relative_path);
            discard_file_or_dir(&full_path).await?;
        }
        "M " | " M" | "MM" | " D" | "D " => {
            // Modified or deleted: restore from HEAD
            run_git(&["restore", "--", &resolved.relative_path], &resolved.repo_root)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))))?;
        }
        "A " | "AM" => {
            // Added: unstage
            run_git(&["reset", "HEAD", "--", &resolved.relative_path], &resolved.repo_root)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))))?;
        }
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("Unsupported status for discard: {}", status)})),
            ));
        }
    }

    Ok(Json(json!({
        "success": true,
        "message": format!("Changes discarded for {}", resolved.relative_path)
    })))
}

// ── 20. POST /delete-untracked ─────────────────────────────────────────────
/// Delete an untracked file or directory from the working tree.

async fn git_delete_untracked(
    _user: Extension<AuthUser>,
    Json(body): Json<FileBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project_path = get_project_path(&body.project).await?;
    validate_git_repository(&project_path).await?;

    let resolved = resolve_repository_file_path(&project_path, &body.file).await?;

    // Confirm file is untracked
    let (status_stdout, _) = run_git(
        &["status", "--porcelain", "--", &resolved.relative_path],
        &resolved.repo_root,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))))?;

    if status_stdout.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "File is not untracked or does not exist"})),
        ));
    }

    if !status_stdout.starts_with("??") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "File is not untracked. Use discard for tracked files."})),
        ));
    }

    let full_path = Path::new(&resolved.repo_root).join(&resolved.relative_path);
    discard_file_or_dir(&full_path).await?;

    Ok(Json(json!({
        "success": true,
        "message": format!("Untracked file {} deleted successfully", resolved.relative_path)
    })))
}

// ===========================================================================
// INTERNAL HELPERS
// ===========================================================================

/// Delete a file or directory from the filesystem.
async fn discard_file_or_dir(path: &Path) -> Result<(), (StatusCode, Json<Value>)> {
    let meta = tokio::fs::metadata(path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "File not found"})),
            );
        }
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
    })?;

    if meta.is_dir() {
        tokio::fs::remove_dir_all(path).await.map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))
        })?;
    } else {
        tokio::fs::remove_file(path).await.map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()})))
        })?;
    }

    Ok(())
}

/// Classify a remote fetch/push error into a user-friendly (message, details) pair.
fn classify_remote_error(e: &str, default_msg: &str) -> (String, String) {
    let lower = e.to_lowercase();
    if lower.contains("could not resolve hostname") {
        (
            "Network error".to_string(),
            "Unable to connect to remote repository. Check your internet connection.".to_string(),
        )
    } else if lower.contains("does not appear to be a git repository") {
        (
            "Remote not configured".to_string(),
            "No remote repository configured. Add a remote with: git remote add origin <url>"
                .to_string(),
        )
    } else {
        (default_msg.to_string(), e.to_string())
    }
}

/// Classify a pull error into a user-friendly (message, details) pair.
fn classify_pull_error(e: &str) -> (String, String) {
    let lower = e.to_lowercase();
    if lower.contains("conflict") {
        (
            "Merge conflicts detected".to_string(),
            "Pull created merge conflicts. Please resolve conflicts manually in the editor, then commit the changes.".to_string(),
        )
    } else if lower.contains("please commit your changes or stash them") {
        (
            "Uncommitted changes detected".to_string(),
            "Please commit or stash your local changes before pulling.".to_string(),
        )
    } else if lower.contains("could not resolve hostname") {
        (
            "Network error".to_string(),
            "Unable to connect to remote repository. Check your internet connection.".to_string(),
        )
    } else if lower.contains("does not appear to be a git repository") {
        (
            "Remote not configured".to_string(),
            "No remote repository configured. Add a remote with: git remote add origin <url>"
                .to_string(),
        )
    } else if lower.contains("diverged") {
        (
            "Branches have diverged".to_string(),
            "Your local branch and remote branch have diverged. Consider fetching first to review changes.".to_string(),
        )
    } else {
        ("Pull failed".to_string(), e.to_string())
    }
}

/// Classify a push error into a user-friendly (message, details) pair.
fn classify_push_error(e: &str) -> (String, String) {
    let lower = e.to_lowercase();
    if lower.contains("rejected") {
        (
            "Push rejected".to_string(),
            "The remote has newer commits. Pull first to merge changes before pushing."
                .to_string(),
        )
    } else if lower.contains("non-fast-forward") {
        (
            "Non-fast-forward push".to_string(),
            "Your branch is behind the remote. Pull the latest changes first.".to_string(),
        )
    } else if lower.contains("could not resolve hostname") {
        (
            "Network error".to_string(),
            "Unable to connect to remote repository. Check your internet connection.".to_string(),
        )
    } else if lower.contains("does not appear to be a git repository") {
        (
            "Remote not configured".to_string(),
            "No remote repository configured. Add a remote with: git remote add origin <url>"
                .to_string(),
        )
    } else if lower.contains("permission denied") {
        (
            "Authentication failed".to_string(),
            "Permission denied. Check your credentials or SSH keys.".to_string(),
        )
    } else if lower.contains("no upstream branch") {
        (
            "No upstream branch".to_string(),
            "No upstream branch configured. Use: git push --set-upstream origin <branch>"
                .to_string(),
        )
    } else {
        ("Push failed".to_string(), e.to_string())
    }
}

/// Classify a publish error into a user-friendly (message, details) pair.
fn classify_publish_error(e: &str) -> (String, String) {
    let lower = e.to_lowercase();
    if lower.contains("rejected") {
        (
            "Publish rejected".to_string(),
            "The remote branch already exists and has different commits. Use push instead."
                .to_string(),
        )
    } else if lower.contains("could not resolve hostname") {
        (
            "Network error".to_string(),
            "Unable to connect to remote repository. Check your internet connection.".to_string(),
        )
    } else if lower.contains("permission denied") {
        (
            "Authentication failed".to_string(),
            "Permission denied. Check your credentials or SSH keys.".to_string(),
        )
    } else if lower.contains("does not appear to be a git repository") {
        (
            "Remote not configured".to_string(),
            "Remote repository not properly configured. Check your remote URL.".to_string(),
        )
    } else {
        ("Publish failed".to_string(), e.to_string())
    }
}
