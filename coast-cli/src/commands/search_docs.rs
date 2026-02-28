/// `coast search-docs` command — hybrid semantic + keyword docs search.
use anyhow::{bail, Result};
use clap::Args;
use rust_i18n::t;

use coast_core::protocol::{Request, Response, SearchDocsRequest, SearchDocsResult};

/// Arguments for `coast search-docs`.
#[derive(Debug, Args)]
pub struct SearchDocsArgs {
    /// Search query.
    #[arg(required = true, num_args = 1..)]
    pub query: Vec<String>,
}

/// Execute the `coast search-docs` command.
pub async fn execute(args: &SearchDocsArgs) -> Result<()> {
    let query = join_query(&args.query);
    let request = Request::SearchDocs(SearchDocsRequest {
        query: query.clone(),
        limit: None,
        language: Some(crate::i18n_helper::cli_lang().to_string()),
    });

    let response = super::send_request(request).await?;
    match response {
        Response::SearchDocs(resp) => {
            println!(
                "{}",
                t!(
                    "cli.search_docs.hybrid_note",
                    locale = &resp.locale,
                    strategy = &resp.strategy
                )
            );
            if resp.results.is_empty() {
                println!("{}", t!("cli.search_docs.no_results", query = query));
                return Ok(());
            }

            println!(
                "{}",
                t!(
                    "cli.search_docs.results_header",
                    query = query,
                    count = resp.results.len()
                )
            );
            println!("{}", format_results(&resp.results));
            Ok(())
        }
        Response::Error(e) => bail!("{}", e.error),
        _ => bail!("{}", t!("error.unexpected_response")),
    }
}

fn join_query(parts: &[String]) -> String {
    parts.join(" ")
}

/// Format search results for terminal output.
pub fn format_results(results: &[SearchDocsResult]) -> String {
    let mut out = Vec::new();
    for (idx, item) in results.iter().enumerate() {
        out.push(format!("{:>2}. {} — {}", idx + 1, item.heading, item.path));
        out.push(format!("    {}", item.snippet));
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: SearchDocsArgs,
    }

    #[test]
    fn test_search_docs_args_parse() {
        let cli = TestCli::try_parse_from(["test", "shared", "services"]).unwrap();
        assert_eq!(cli.args.query, vec!["shared", "services"]);
    }

    #[test]
    fn test_join_query() {
        let query = join_query(&["shared".to_string(), "services".to_string()]);
        assert_eq!(query, "shared services");
    }

    #[test]
    fn test_format_results() {
        let rendered = format_results(&[SearchDocsResult {
            path: "coastfiles/COASTFILE.md".to_string(),
            route: "/docs/coastfiles/COASTFILE".to_string(),
            heading: "Coastfile".to_string(),
            snippet: "Defines runtime and services.".to_string(),
            score: 1.0,
        }]);

        assert!(rendered.contains("Coastfile"));
        assert!(rendered.contains("coastfiles/COASTFILE.md"));
        assert!(rendered.contains("Defines runtime and services."));
    }
}
