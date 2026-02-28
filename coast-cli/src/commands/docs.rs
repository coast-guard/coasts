/// `coast docs` command — list docs tree or print markdown content.
use anyhow::{bail, Result};
use clap::Args;
use rust_i18n::t;

use coast_core::protocol::{DocsNode, DocsNodeKind, DocsRequest, Request, Response};

/// Arguments for `coast docs`.
#[derive(Debug, Args)]
pub struct DocsArgs {
    /// Optional docs path. When set, prints the resolved markdown content.
    #[arg(long)]
    pub path: Option<String>,
}

/// Execute the `coast docs` command.
pub async fn execute(args: &DocsArgs) -> Result<()> {
    let request = Request::Docs(DocsRequest {
        path: args.path.clone(),
        language: Some(crate::i18n_helper::cli_lang().to_string()),
    });

    let response = super::send_request(request).await?;

    match response {
        Response::Docs(resp) => {
            if let Some(content) = resp.content {
                println!("{content}");
                return Ok(());
            }

            if resp.tree.is_empty() {
                println!("{}", t!("cli.docs.no_docs_found"));
                return Ok(());
            }

            println!("{}", t!("cli.docs.tree_header", locale = &resp.locale));
            println!("{}", format_docs_tree(&resp.tree));
            Ok(())
        }
        Response::Error(e) => bail!("{}", e.error),
        _ => bail!("{}", t!("error.unexpected_response")),
    }
}

/// Format docs tree for terminal output.
pub fn format_docs_tree(nodes: &[DocsNode]) -> String {
    let mut lines = Vec::new();
    append_tree(nodes, 0, &mut lines);
    lines.join("\n")
}

fn append_tree(nodes: &[DocsNode], depth: usize, out: &mut Vec<String>) {
    let indent = "  ".repeat(depth);
    for node in nodes {
        match node.kind {
            DocsNodeKind::Dir => {
                out.push(format!("{indent}{}/", node.name));
                append_tree(&node.children, depth + 1, out);
            }
            DocsNodeKind::File => out.push(format!("{indent}{}", node.name)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: DocsArgs,
    }

    #[test]
    fn test_docs_args_parse_without_path() {
        let cli = TestCli::try_parse_from(["test"]).unwrap();
        assert_eq!(cli.args.path, None);
    }

    #[test]
    fn test_docs_args_parse_with_path() {
        let cli = TestCli::try_parse_from(["test", "--path", "coastfiles/COASTFILE"]).unwrap();
        assert_eq!(cli.args.path.as_deref(), Some("coastfiles/COASTFILE"));
    }

    #[test]
    fn test_format_docs_tree() {
        let tree = vec![
            DocsNode {
                name: "README.md".to_string(),
                path: "README.md".to_string(),
                kind: DocsNodeKind::File,
                children: Vec::new(),
            },
            DocsNode {
                name: "coastfiles".to_string(),
                path: "coastfiles".to_string(),
                kind: DocsNodeKind::Dir,
                children: vec![DocsNode {
                    name: "COASTFILE.md".to_string(),
                    path: "coastfiles/COASTFILE.md".to_string(),
                    kind: DocsNodeKind::File,
                    children: Vec::new(),
                }],
            },
        ];

        let rendered = format_docs_tree(&tree);
        assert!(rendered.contains("README.md"));
        assert!(rendered.contains("coastfiles/"));
        assert!(rendered.contains("  COASTFILE.md"));
    }
}
