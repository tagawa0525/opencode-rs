//! Question tool - allows LLM to ask users questions during execution.
//!
//! This tool enables interactive workflows where the LLM can gather user
//! preferences, clarify ambiguous instructions, or get decisions on implementation choices.

use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const DESCRIPTION: &str = r#"Use this tool when you need to ask the user questions during execution. This allows you to:
1. Gather user preferences or requirements
2. Clarify ambiguous instructions
3. Get decisions on implementation choices as you work
4. Offer choices to the user about what direction to take.

Usage notes:
- When `custom` is enabled (default), a "Type your own answer" option is added automatically; don't include "Other" or catch-all options
- Answers are returned as arrays of labels; set `multiple: true` to allow selecting more than one
- If you recommend a specific option, make that the first option in the list and add "(Recommended)" at the end of the label
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionInfo {
    pub question: String,
    pub header: String,
    pub options: Vec<QuestionOption>,
    #[serde(default)]
    pub multiple: bool,
    #[serde(default = "default_custom")]
    pub custom: bool,
}

fn default_custom() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionParams {
    pub questions: Vec<QuestionInfo>,
}

pub struct QuestionTool;

#[async_trait::async_trait]
impl Tool for QuestionTool {
    fn id(&self) -> &str {
        "question"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "question".to_string(),
            description: DESCRIPTION.to_string(),
            parameters: json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": {
                    "questions": {
                        "description": "Questions to ask",
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "question": {
                                    "type": "string",
                                    "description": "Complete question"
                                },
                                "header": {
                                    "type": "string",
                                    "maxLength": 12,
                                    "description": "Very short label (max 12 chars)"
                                },
                                "options": {
                                    "type": "array",
                                    "description": "Available choices",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "label": {
                                                "type": "string",
                                                "description": "Display text (1-5 words, concise)"
                                            },
                                            "description": {
                                                "type": "string",
                                                "description": "Explanation of choice"
                                            }
                                        },
                                        "required": ["label", "description"],
                                        "additionalProperties": false
                                    }
                                },
                                "multiple": {
                                    "type": "boolean",
                                    "description": "Allow selecting multiple choices"
                                },
                                "custom": {
                                    "type": "boolean",
                                    "description": "Allow typing a custom answer (default: true)"
                                }
                            },
                            "required": ["question", "header", "options"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["questions"]
            }),
        }
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let params: QuestionParams = serde_json::from_value(args)?;

        // TODO: Implement actual user interaction through event bus
        // For now, return a placeholder that explains the feature is not yet implemented

        // In the TypeScript version, this would:
        // 1. Publish a question.asked event with the questions
        // 2. Wait for a question.replied event with the answers
        // 3. Return the answers to the LLM

        // For now, we'll return empty answers as a placeholder
        let answers: Vec<Vec<String>> = params.questions.iter().map(|_| Vec::new()).collect();

        let formatted = params
            .questions
            .iter()
            .zip(&answers)
            .map(|(q, a)| {
                let answer_str = if a.is_empty() {
                    "Unanswered".to_string()
                } else {
                    a.join(", ")
                };
                format!("\"{}\"=\"{}\"", q.question, answer_str)
            })
            .collect::<Vec<_>>()
            .join(", ");

        let mut metadata = HashMap::new();
        metadata.insert("answers".to_string(), serde_json::to_value(&answers)?);
        metadata.insert(
            "note".to_string(),
            Value::String("Question tool not yet fully implemented in Rust version".to_string()),
        );

        let question_count = params.questions.len();
        Ok(ToolResult {
            title: format!(
                "Asked {} question{}",
                question_count,
                if question_count > 1 { "s" } else { "" }
            ),
            output: format!(
                "User has answered your questions: {}. You can now continue with the user's answers in mind.\n\n\
                Note: The question tool is not yet fully implemented. Interactive question prompts will be added in a future update.",
                formatted
            ),
            metadata,
            truncated: false,
            attachments: Vec::new(),
        })
    }
}
