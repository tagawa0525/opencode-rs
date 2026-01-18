use crate::slash_command::{CommandContext, CommandOutput, SlashCommand};
use anyhow::Result;
use async_trait::async_trait;

/// Review command - review changes (commit/branch/pr)
pub struct ReviewCommand;

#[async_trait]
impl SlashCommand for ReviewCommand {
    fn name(&self) -> &str {
        "review"
    }

    fn description(&self) -> &str {
        "Review changes [commit|branch|pr]"
    }

    fn usage(&self) -> &str {
        "/review [commit|branch|pr] [id]"
    }

    async fn execute(&self, args: &str, _ctx: &CommandContext) -> Result<CommandOutput> {
        let args = args.trim();

        let prompt = if args.is_empty() {
            // Review current changes
            r#"Please review the current git changes.

Run `git status` and `git diff` to see what has changed.
Provide feedback on:
- Code quality and style
- Potential bugs or issues
- Suggestions for improvement
- Test coverage"#
                .to_string()
        } else {
            // Parse arguments
            let parts: Vec<&str> = args.split_whitespace().collect();
            match parts.first() {
                Some(&"commit") => {
                    let commit_id = parts.get(1).unwrap_or(&"HEAD");
                    format!(
                        r#"Please review the git commit: {}

Run `git show {}` to see the changes.
Provide feedback on the code changes."#,
                        commit_id, commit_id
                    )
                }
                Some(&"branch") => {
                    let branch = parts.get(1).unwrap_or(&"HEAD");
                    format!(
                        r#"Please review the git branch: {}

Run `git diff main...{}` to see the changes.
Provide feedback on all changes in this branch."#,
                        branch, branch
                    )
                }
                Some(&"pr") => {
                    let pr_number = parts.get(1).unwrap_or(&"");
                    if pr_number.is_empty() {
                        "Please specify a PR number. Usage: /review pr <number>".to_string()
                    } else {
                        format!(
                            r#"Please review pull request #{}

Use `gh pr view {} --json title,body,commits,files` to get PR details.
Provide feedback on the changes."#,
                            pr_number, pr_number
                        )
                    }
                }
                _ => {
                    "Usage: /review [commit|branch|pr] [id]\n\nExamples:\n- /review\n- /review commit HEAD\n- /review branch feature-branch\n- /review pr 123".to_string()
                }
            }
        };

        Ok(CommandOutput::prompt(prompt))
    }
}
