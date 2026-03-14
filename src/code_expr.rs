use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CodeExpr {
    Literal(Value),
    Mini(String),
    Call {
        func: String,
        args: Vec<CodeExpr>,
    },
    Method {
        receiver: Box<CodeExpr>,
        method: String,
        args: Vec<CodeExpr>,
    },
    Ident(String),
    Raw(String),
}

impl CodeExpr {
    pub fn into_mini_if_string(self) -> Self {
        match self {
            CodeExpr::Literal(Value::String(value)) => CodeExpr::Mini(value),
            other => other,
        }
    }

    pub fn render(&self) -> String {
        match self {
            CodeExpr::Literal(value) => match value {
                Value::String(v) => {
                    if v.trim().is_empty() {
                        "".to_string()
                    } else {
                        render_js_string(v, '\'')
                    }
                }
                other => other.to_string(),
            },
            CodeExpr::Mini(v) => {
                if v.trim().is_empty() {
                    "".to_string()
                } else {
                    render_js_string(v, '"')
                }
            }
            CodeExpr::Call { func, args } => {
                let rendered = args
                    .iter()
                    .map(CodeExpr::render)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{func}({rendered})")
            }
            CodeExpr::Method {
                receiver,
                method,
                args,
            } => {
                let rendered = args
                    .iter()
                    .map(CodeExpr::render)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}.{}({})", receiver.render(), method, rendered)
            }
            CodeExpr::Ident(name) => name.clone(),
            CodeExpr::Raw(raw) => raw.clone(),
        }
    }
}

fn render_js_string(value: &str, quote: char) -> String {
    let mut rendered = String::with_capacity(value.len() + 2);
    rendered.push(quote);
    for ch in value.chars() {
        match ch {
            '\\' => rendered.push_str("\\\\"),
            '\'' if quote == '\'' => rendered.push_str("\\'"),
            '"' if quote == '"' => rendered.push_str("\\\""),
            '\n' => rendered.push_str("\\n"),
            '\r' => rendered.push_str("\\r"),
            '\t' => rendered.push_str("\\t"),
            _ => rendered.push(ch),
        }
    }
    rendered.push(quote);
    rendered
}
