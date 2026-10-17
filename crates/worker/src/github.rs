// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! GitHub as the App `suhina-bot` (`suhina-bot[bot]`). The App's JWT is signed with WebCrypto;
//! the installation token it buys lives an hour and is kept in the bot's state for 50 minutes, so most
//! commands sign nothing.

use serde_json::{Value, json};
use suhina_bot_core::github as core;
use suhina_bot_core::reply::shorten;
use web_sys::{CryptoKey, WorkerGlobalScope};
use worker::js_sys::{Array, Object, Reflect, Uint8Array};
use worker::wasm_bindgen::{JsCast, JsValue};
use worker::wasm_bindgen_futures::JsFuture;
use worker::{Date, Env, Error, Fetch, Headers, Method, Request, RequestInit, Result};

use crate::settings;
use crate::store::Store;

const API: &str = "https://api.github.com";
const TOKEN_KEY: &str = "github-token";
const TOKEN_TTL_SECONDS: u64 = 50 * 60;
const USER_AGENT: &str = "suhina-bot (+https://github.com/suhina-daw/suhina-bot)";

pub struct GitHub {
    env: Env,
    repository: String,
}

impl GitHub {
    pub fn new(env: &Env) -> Result<Self> {
        Ok(Self {
            env: env.clone(),
            repository: settings::require(env, "GITHUB_REPOSITORY")?,
        })
    }

    /// `owner/name` of the repository the App works on.
    pub fn repository(&self) -> &str {
        &self.repository
    }

    /// Open issues and pull requests of the repository.
    pub async fn open_issue_count(&self) -> Result<u64> {
        let repo = self
            .rest(Method::Get, &format!("/repos/{}", self.repository), None)
            .await?;
        repo.get("open_issues_count")
            .and_then(Value::as_u64)
            .ok_or_else(|| Error::RustError("repository answer has no open_issues_count".into()))
    }

    /// The latest nightly release, or `None` when there is none with packages.
    pub async fn latest_nightly(&self) -> Result<Option<suhina_bot_core::nightly::Nightly>> {
        let path = format!(
            "/repos/{}/releases/tags/{}",
            self.repository,
            suhina_bot_core::nightly::TAG
        );
        let release = self.rest(Method::Get, &path, None).await?;
        Ok(suhina_bot_core::nightly::Nightly::from_release(&release))
    }

    /// Opens an issue; returns its number, link and node id. When GitHub refuses the issue type
    /// (an organization without it), the issue is opened without one.
    pub async fn create_issue(
        &self,
        title: &str,
        body: &str,
        labels: &[String],
        issue_type: &str,
    ) -> Result<(u64, String, String)> {
        let path = format!("/repos/{}/issues", self.repository);
        let mut payload =
            json!({ "title": title, "body": body, "labels": labels, "type": issue_type });
        // Retried without the type only when GitHub refused the type itself (422 naming it); any
        // other failure may have created the issue already, and a retry would open a second.
        let issue = match self.rest(Method::Post, &path, Some(&payload)).await {
            Ok(issue) => issue,
            Err(Error::RustError(message))
                if message.starts_with("GitHub answered 422") && message.contains("type") =>
            {
                if let Some(object) = payload.as_object_mut() {
                    object.remove("type");
                }
                self.rest(Method::Post, &path, Some(&payload)).await?
            }
            Err(error) => return Err(error),
        };
        let number = issue.get("number").and_then(Value::as_u64);
        let url = issue.get("html_url").and_then(Value::as_str);
        let node = issue.get("node_id").and_then(Value::as_str);
        match (number, url, node) {
            (Some(number), Some(url), Some(node)) => Ok((number, url.to_owned(), node.to_owned())),
            _ => Err(Error::RustError(
                "issue answer without number, link or id".into(),
            )),
        }
    }

    /// Up to five open issues whose titles share words with `title`.
    pub async fn similar(&self, title: &str) -> Result<Vec<suhina_bot_core::review::Similar>> {
        let Some(query) = suhina_bot_core::review::similar_query(&self.repository, title) else {
            return Ok(Vec::new());
        };
        let encoded: String = worker::js_sys::encode_uri_component(&query).into();
        let found = self
            .rest(
                Method::Get,
                &format!("/search/issues?per_page=5&q={encoded}"),
                None,
            )
            .await?;
        Ok(found
            .get("items")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        Some(suhina_bot_core::review::Similar {
                            number: item.get("number")?.as_u64()?,
                            title: shorten(item.get("title")?.as_str()?, 80),
                            url: item.get("html_url")?.as_str()?.to_owned(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// The project board's fields and options.
    pub async fn board(&self, owner: &str, number: u64) -> Result<core::Board> {
        let data = self
            .graphql(
                core::BOARD_QUERY,
                &json!({ "owner": owner, "number": number }),
            )
            .await?;
        core::Board::parse(&data)
            .ok_or_else(|| Error::RustError("board answer without a project".into()))
    }

    /// Puts an issue on the board in `Backlog`, with Priority and Phase when given.
    pub async fn add_to_board(
        &self,
        board: &core::Board,
        issue_node: &str,
        priority: Option<&str>,
        phase: Option<&str>,
    ) -> Result<()> {
        let added = self
            .graphql(
                "mutation($project:ID!,$content:ID!){addProjectV2ItemById(input:{projectId:$project,contentId:$content}){item{id}}}",
                &json!({ "project": board.id, "content": issue_node }),
            )
            .await?;
        let item = added
            .pointer("/addProjectV2ItemById/item/id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::RustError("board answer without an item".into()))?
            .to_owned();
        let wanted = [
            ("Status", Some("Backlog")),
            ("Priority", priority),
            ("Phase", phase),
        ];
        for (field, option) in wanted {
            let Some((field_id, option_id)) = option.and_then(|o| board.option(field, o)) else {
                continue;
            };
            self.graphql(
                "mutation($project:ID!,$item:ID!,$field:ID!,$option:String!){updateProjectV2ItemFieldValue(input:{projectId:$project,itemId:$item,fieldId:$field,value:{singleSelectOptionId:$option}}){projectV2Item{id}}}",
                &json!({ "project": board.id, "item": item, "field": field_id, "option": option_id }),
            )
            .await?;
        }
        Ok(())
    }

    /// A REST call as the installation. Answers other than 2xx are errors with GitHub's message.
    pub async fn rest(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Value> {
        let token = self.installation_token().await?;
        let (status, bytes) = send(
            method,
            &format!("{API}{path}"),
            &format!("token {token}"),
            body,
        )
        .await?;
        parse_answer(status, &bytes)
    }

    /// A GraphQL query as the installation; GraphQL errors are errors too.
    pub async fn graphql(&self, query: &str, variables: &Value) -> Result<Value> {
        let answer = self
            .rest(
                Method::Post,
                "/graphql",
                Some(&json!({ "query": query, "variables": variables })),
            )
            .await?;
        if let Some(errors) = answer.get("errors") {
            return Err(Error::RustError(format!(
                "GraphQL: {}",
                shorten(&errors.to_string(), 300)
            )));
        }
        Ok(answer.get("data").cloned().unwrap_or(Value::Null))
    }

    async fn installation_token(&self) -> Result<String> {
        let store = Store::new(&self.env)?;
        if let Some(token) = store.get::<String>(TOKEN_KEY).await? {
            return Ok(token);
        }

        let jwt = self.app_jwt().await?;
        let auth = format!("Bearer {jwt}");
        let (status, bytes) = send(
            Method::Get,
            &format!("{API}/repos/{}/installation", self.repository),
            &auth,
            None,
        )
        .await?;
        parse_answer(status, &bytes)?;
        let installation = core::parse_installation(&bytes).map_err(core_error)?;

        let url = format!("{API}/app/installations/{}/access_tokens", installation.id);
        let (status, bytes) = send(Method::Post, &url, &auth, None).await?;
        parse_answer(status, &bytes)?;
        let token = core::parse_installation_token(&bytes)
            .map_err(core_error)?
            .token;

        store
            .put(TOKEN_KEY, &token, Some(TOKEN_TTL_SECONDS))
            .await?;
        Ok(token)
    }

    async fn app_jwt(&self) -> Result<String> {
        let app_id = settings::require(&self.env, "GITHUB_APP_ID")?;
        let pem = settings::require(&self.env, "GITHUB_APP_PRIVATE_KEY")?;
        let input = core::jwt_signing_input(&app_id, Date::now().as_millis() / 1000);
        let signature = sign_rs256(&pem, input.as_bytes()).await?;
        Ok(core::jwt(&input, &signature))
    }
}

/// RS256 with WebCrypto: import the PKCS#8 key and sign.
async fn sign_rs256(pem: &str, data: &[u8]) -> Result<Vec<u8>> {
    let der = core::pkcs8_der(pem).map_err(core_error)?;
    let global: WorkerGlobalScope = worker::js_sys::global().unchecked_into();
    let subtle = global.crypto()?.subtle();

    let algorithm = Object::new();
    Reflect::set(&algorithm, &"name".into(), &"RSASSA-PKCS1-v1_5".into())?;
    Reflect::set(&algorithm, &"hash".into(), &"SHA-256".into())?;
    let usages = Array::of1(&JsValue::from_str("sign"));
    let key_data = Uint8Array::from(der.as_slice());

    let key: CryptoKey = JsFuture::from(
        subtle.import_key_with_object("pkcs8", &key_data, &algorithm, false, &usages)?,
    )
    .await?
    .unchecked_into();
    let signature =
        JsFuture::from(subtle.sign_with_str_and_u8_array("RSASSA-PKCS1-v1_5", &key, data)?).await?;
    Ok(Uint8Array::new(&signature).to_vec())
}

async fn send(
    method: Method,
    url: &str,
    authorization: &str,
    body: Option<&Value>,
) -> Result<(u16, Vec<u8>)> {
    let headers = Headers::new();
    headers.set("Authorization", authorization)?;
    headers.set("Accept", "application/vnd.github+json")?;
    headers.set("X-GitHub-Api-Version", "2022-11-28")?;
    headers.set("User-Agent", USER_AGENT)?;
    let mut init = RequestInit::new();
    init.with_method(method).with_headers(headers);
    if let Some(body) = body {
        init.headers.set("Content-Type", "application/json")?;
        init.with_body(Some(JsValue::from_str(&body.to_string())));
    }
    let mut response = Fetch::Request(Request::new_with_init(url, &init)?)
        .send()
        .await?;
    Ok((response.status_code(), response.bytes().await?))
}

fn parse_answer(status: u16, bytes: &[u8]) -> Result<Value> {
    if !(200..300).contains(&status) {
        let text = String::from_utf8_lossy(bytes);
        return Err(Error::RustError(format!(
            "GitHub answered {status}: {}",
            shorten(&text, 300)
        )));
    }
    if bytes.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(bytes)
        .map_err(|_| Error::RustError(format!("GitHub answered {status} with no JSON")))
}

fn core_error(error: suhina_bot_core::Error) -> Error {
    Error::RustError(error.to_string())
}
