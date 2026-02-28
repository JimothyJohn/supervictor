# Worktree Fix Workflow

You are executing a structured worktree-based fix workflow. Follow these steps exactly:

## 1. Create Worktree
- Create a worktree named after the task (short, kebab-case)
- Confirm you're working in the isolated worktree

## 2. Do the Work
Task: $ARGUMENTS

- Read and understand the relevant code before making changes
- Make the fix/change
- Run tests to verify (`uv run pytest` for Python, `cargo test` for Rust)
- If tests fail, fix and re-run until green

## 3. Report Results
Show the user:
- What changed (files modified/created)
- Test results (pass count)

Then ask: **"Tests pass. Ready to merge changes into `<source-branch>`?"**

## 4. On Approval
- Copy changed/new files from the worktree into the main repo (overwrite in place)
- Remove the worktree: `git -C <main-repo-path> worktree remove <worktree-path>`
- Confirm: "Changes merged, worktree cleaned up. Files are unstaged for your review."

Do NOT `git add`, `git commit`, or `git push`. The user will handle that.

## 5. On Rejection
- Ask what needs to change
- Go back to step 2
