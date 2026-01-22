//! Session transcript formatting utilities.

use crate::tui::types::DisplayMessage;

/// Options for formatting transcripts
pub struct TranscriptOptions {
    pub include_tool_details: bool,
}

impl Default for TranscriptOptions {
    fn default() -> Self {
        Self {
            include_tool_details: true,
        }
    }
}

/// Format session messages as a markdown transcript
pub fn format_transcript(
    session_title: &str,
    session_id: &str,
    messages: &[DisplayMessage],
    options: &TranscriptOptions,
) -> String {
    let mut output = String::new();

    // Header
    output.push_str(&format!("# {}\n\n", session_title));
    output.push_str(&format!("Session ID: `{}`\n", session_id));
    output.push_str(&format!(
        "Exported: {}\n\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    ));
    output.push_str("---\n\n");

    // Messages
    for msg in messages {
        match msg.role.as_str() {
            "user" => {
                output.push_str("## User\n\n");
                output.push_str(&format_message_content(&msg.parts, options));
                output.push_str("\n\n");
            }
            "assistant" => {
                output.push_str("## Assistant\n\n");
                output.push_str(&format_message_content(&msg.parts, options));
                output.push_str("\n\n");
            }
            "system" => {
                output.push_str("## System\n\n");
                output.push_str(&format_message_content(&msg.parts, options));
                output.push_str("\n\n");
            }
            _ => {
                output.push_str(&format!("## {}\n\n", msg.role));
                output.push_str(&format_message_content(&msg.parts, options));
                output.push_str("\n\n");
            }
        }
    }

    output
}

/// Format message parts into text
fn format_message_content(
    parts: &[crate::tui::types::MessagePart],
    options: &TranscriptOptions,
) -> String {
    let mut content = String::new();

    for part in parts {
        use crate::tui::types::MessagePart;
        match part {
            MessagePart::Text { text } => {
                content.push_str(text);
            }
            MessagePart::ToolCall { name, args } => {
                if options.include_tool_details {
                    content.push_str(&format!(
                        "\n**Tool Call: {}**\n```json\n{}\n```\n",
                        name, args
                    ));
                }
            }
            MessagePart::ToolResult { output, is_error } => {
                if options.include_tool_details {
                    let status = if *is_error { "Error" } else { "Success" };
                    content.push_str(&format!(
                        "\n**Tool Result ({}):**\n```\n{}\n```\n",
                        status, output
                    ));
                }
            }
        }
    }

    content
}
