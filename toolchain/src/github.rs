use serde::{Deserialize, Serialize};
use reqwest::Client;
use anyhow::{Result, anyhow};

#[derive(Debug, Deserialize)]
struct GithubResponse {
    data: Data,
}

#[derive(Debug, Deserialize)]
struct Data {
    search: Search,
}

#[derive(Debug, Deserialize)]
struct Search {
    nodes: Vec<RepositoryNode>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")] // Handling generic nodes if needed, but simple structure works for specific query
struct RepositoryNode {
    name: String,
    owner: Owner,
    url: String,
    description: Option<String>,
    #[serde(rename = "defaultBranchRef")]
    default_branch_ref: Option<BranchRef>,
}

#[derive(Debug, Deserialize)]
struct Owner {
    login: String,
}

#[derive(Debug, Deserialize)]
struct BranchRef {
    name: String,
}

#[derive(Debug, Clone)]
pub struct PackageInfo {
    pub owner: String,
    pub name: String,
    pub url: String,
    pub description: String,
    pub default_branch: String,
}

pub struct GithubClient {
    client: Client,
    token: Option<String>,
}

impl GithubClient {
    pub fn new(token: Option<String>) -> Result<Self> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::USER_AGENT, 
            reqwest::header::HeaderValue::from_static("lunu-cli/1.0")
        );
        
        if let Some(ref t) = token {
            let mut auth_val = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", t))?;
            auth_val.set_sensitive(true);
            headers.insert(reqwest::header::AUTHORIZATION, auth_val);
        }

        let client = Client::builder()
            .default_headers(headers)
            .http2_prior_knowledge() // Optimize for HTTP/2
            .build()?;

        Ok(Self { client, token })
    }

    pub async fn search_packages(&self, query: &str) -> Result<Vec<PackageInfo>> {
        let gql_query = r#"
        query SearchRepos($q: String!) {
            search(query: $q, type: REPOSITORY, first: 10) {
                nodes {
                    ... on Repository {
                        name
                        owner { login }
                        url
                        description
                        defaultBranchRef { name }
                    }
                }
            }
        }
        "#;

        let payload = serde_json::json!({
            "query": gql_query,
            "variables": {
                "q": query
            }
        });

        // Use public endpoint if no token, but GraphQL often requires token.
        // Fallback to REST search if GraphQL fails auth or try public access.
        // GitHub GraphQL API requires authentication.
        if self.token.is_none() {
             return self.search_rest(query).await;
        }

        let res = self.client.post("https://api.github.com/graphql")
            .json(&payload)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(anyhow!("GitHub API Error: {}", res.status()));
        }

        let body: GithubResponse = res.json().await?;
        
        let packages = body.data.search.nodes.into_iter().map(|node| PackageInfo {
            owner: node.owner.login,
            name: node.name,
            url: node.url,
            description: node.description.unwrap_or_default(),
            default_branch: node.default_branch_ref.map(|b| b.name).unwrap_or_else(|| "main".to_string()),
        }).collect();

        Ok(packages)
    }

    async fn search_rest(&self, query: &str) -> Result<Vec<PackageInfo>> {
        // Fallback for unauthenticated users
        #[derive(Deserialize)]
        struct RestSearch { items: Vec<RestRepo> }
        #[derive(Deserialize)]
        struct RestRepo { 
            name: String, 
            owner: Owner, 
            html_url: String, 
            description: Option<String>, 
            default_branch: String 
        }

        let res = self.client.get("https://api.github.com/search/repositories")
            .query(&[("q", query), ("per_page", "10")])
            .send()
            .await?;

        let body: RestSearch = res.json().await?;
        
        Ok(body.items.into_iter().map(|item| PackageInfo {
            owner: item.owner.login,
            name: item.name,
            url: item.html_url,
            description: item.description.unwrap_or_default(),
            default_branch: item.default_branch,
        }).collect())
    }
}
