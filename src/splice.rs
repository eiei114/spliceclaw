use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::Serialize;
use serde_json::{Value, json};

use crate::error::AppError;
use crate::firefox::Session;

const GRAPHQL: &str = "https://surfaces-graphql.splice.com/graphql";
const MAX_DOWNLOAD_BYTES: u64 = 40_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MediaChoice {
    Preview,
    Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourcePlan {
    Preview,
    Source,
    PurchaseThenSource,
    NeedConfirm,
}

pub(crate) fn plan_source(
    choice: MediaChoice,
    licensed: bool,
    has_source: bool,
    confirm: bool,
) -> SourcePlan {
    if choice == MediaChoice::Preview {
        return SourcePlan::Preview;
    }
    if has_source {
        return SourcePlan::Source;
    }
    if licensed {
        return SourcePlan::Source;
    }
    if confirm {
        SourcePlan::PurchaseThenSource
    } else {
        SourcePlan::NeedConfirm
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SampleHit {
    pub catalog_uuid: String,
    pub file_hash: String,
    pub name: String,
    pub duration_ms: Option<u64>,
    pub category: String,
    pub licensed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credit_cost: Option<u64>,
    pub page_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_url: Option<String>,
}

#[derive(Debug)]
pub(crate) struct AssetFiles {
    pub name: String,
    pub licensed: bool,
    pub preview_url: Option<String>,
    pub source_url: Option<String>,
    pub source_kind: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Account {
    pub username: String,
    pub subscription_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credits: Option<u64>,
}

pub(crate) fn user_agent() -> &'static str {
    match std::env::consts::OS {
        "macos" => {
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:143.0) Gecko/20100101 Firefox/143.0"
        }
        "windows" => {
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:143.0) Gecko/20100101 Firefox/143.0"
        }
        _ => "Mozilla/5.0 (X11; Linux x86_64; rv:143.0) Gecko/20100101 Firefox/143.0",
    }
}

pub(crate) fn fetch_account(session: &Session) -> Result<Account, AppError> {
    let body = graphql(
        session,
        r#"query Account { user { username } subscription { credits state } }"#,
        json!({}),
    )?;
    let username = body
        .pointer("/data/user/username")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    if username.is_empty() || username.len() > 80 {
        return Err(unexpected());
    }
    let state = body
        .pointer("/data/subscription/state")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let credits = body
        .pointer("/data/subscription/credits")
        .and_then(Value::as_u64);
    Ok(Account {
        username,
        subscription_state: state,
        credits,
    })
}

pub(crate) fn search(
    session: &Session,
    query: &str,
    category: Option<&str>,
    limit: u8,
) -> Result<(u64, Vec<SampleHit>), AppError> {
    let body = graphql(
        session,
        r#"
        query SamplesSearch($query: String!, $limit: Int!, $category: AssetCategorySlug) {
          assetsSearch(
            filter: {
              legacy: true
              published: true
              asset_type_slug: sample
              query: $query
              asset_category_slug: $category
            }
            pagination: { page: 1, limit: $limit }
            sort: { sort: popularity, order: DESC }
          ) {
            items {
              ... on SampleAsset {
                uuid
                catalog_uuid
                name
                duration
                asset_category_slug
                licensed
                asset_prices { amount currency }
                files { asset_file_type_slug url }
              }
            }
            response_metadata { records }
          }
        }
        "#,
        json!({ "query": query, "limit": limit, "category": category }),
    )?;
    let search = body.pointer("/data/assetsSearch").ok_or_else(unexpected)?;
    let records = search
        .pointer("/response_metadata/records")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let items = search
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(unexpected)?;
    let mut hits = Vec::new();
    for item in items {
        if let Some(hit) = sample_hit(item) {
            hits.push(hit);
        }
    }
    Ok((records, hits))
}

pub(crate) fn fetch_asset(session: &Session, catalog_uuid: &str) -> Result<AssetFiles, AppError> {
    let body = graphql(
        session,
        r#"
        query SampleAssetFiles($uuid: GUID!) {
          asset(uuid: $uuid) {
            __typename
            ... on SampleAsset {
              name
              licensed
              files(includeSource: true) { asset_file_type_slug url name }
            }
          }
        }
        "#,
        json!({ "uuid": catalog_uuid }),
    )?;
    let asset = body.pointer("/data/asset").ok_or_else(unexpected)?;
    if asset.is_null() {
        return Err(AppError::new(
            "sample_missing",
            "Splice did not return that sample.",
            "Use the catalog UUID from spliceclaw search.",
        ));
    }
    let name = asset
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("sample")
        .to_owned();
    let licensed = asset
        .get("licensed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let files = asset.get("files").and_then(Value::as_array);
    let (preview_url, source_url, source_kind) = classify_files(files);
    Ok(AssetFiles {
        name,
        licensed,
        preview_url,
        source_url,
        source_kind,
    })
}

pub(crate) fn purchase(session: &Session, catalog_uuid: &str) -> Result<(), AppError> {
    let body = graphql(
        session,
        r#"
        mutation PurchaseAsset($uuid: GUID!) {
          purchaseAsset(uuid: $uuid) {
            ... on IAsset { uuid licensed }
          }
        }
        "#,
        json!({ "uuid": catalog_uuid }),
    )?;
    let licensed = body
        .pointer("/data/purchaseAsset/licensed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !licensed {
        return Err(AppError::new(
            "purchase_failed",
            "Splice did not license the sample.",
            "Check the remaining credits in spliceclaw auth status --live. No file was saved.",
        ));
    }
    Ok(())
}

pub(crate) fn download_file(
    url: &str,
    directory: &Path,
    file_name: &str,
) -> Result<PathBuf, AppError> {
    if !https_asset_url(url) {
        return Err(AppError::new(
            "download_url_rejected",
            "The sample URL is not a Splice asset host.",
            "Stop. The URL was discarded.",
        ));
    }
    std::fs::create_dir_all(directory).map_err(|_| output_error())?;
    let root = directory.canonicalize().map_err(|_| output_error())?;
    let destination = root.join(file_name);
    if destination.parent() != Some(root.as_path()) {
        return Err(output_error());
    }
    if destination.exists() {
        return Err(AppError::new(
            "output_exists",
            "The destination file already exists.",
            "Move the existing file or choose another --out directory.",
        ));
    }
    let temporary = root.join(format!(".{file_name}.partial"));
    let client = download_client()?;
    let mut response = client.get(url).send().map_err(|_| unreachable_error())?;
    let status = response.status();
    if !status.is_success() {
        let _ = std::fs::remove_file(&temporary);
        return Err(AppError::http(
            "download_failed",
            "Splice did not return the audio file.",
            "The link may have expired. Run the command once more. The body was discarded.",
            status.as_u16(),
        ));
    }
    let mut file = std::fs::File::create(&temporary).map_err(|_| output_error())?;
    let mut buffer = [0_u8; 16_384];
    let mut total = 0_u64;
    loop {
        let read = response.read(&mut buffer).map_err(|_| {
            let _ = std::fs::remove_file(&temporary);
            unreachable_error()
        })?;
        if read == 0 {
            break;
        }
        total += u64::try_from(read).unwrap_or(u64::MAX);
        if total > MAX_DOWNLOAD_BYTES {
            let _ = std::fs::remove_file(&temporary);
            return Err(AppError::new(
                "download_too_large",
                "The audio file is larger than this CLI will save.",
                "Stop. The partial file was removed.",
            ));
        }
        file.write_all(&buffer[..read]).map_err(|_| {
            let _ = std::fs::remove_file(&temporary);
            output_error()
        })?;
    }
    file.sync_all().ok();
    drop(file);
    std::fs::rename(&temporary, &destination).map_err(|_| {
        let _ = std::fs::remove_file(&temporary);
        output_error()
    })?;
    Ok(destination)
}

pub(crate) fn output_file_name(sample_name: &str, preview: bool) -> String {
    let base = sample_name.rsplit(['/', '\\']).next().unwrap_or("sample");
    let mut clean = String::new();
    for character in base.chars() {
        if clean.len() >= 80 {
            break;
        }
        if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
            clean.push(character);
        } else {
            clean.push('_');
        }
    }
    let mut clean = clean.trim_matches('_').to_owned();
    if clean.is_empty() {
        clean = "sample".to_owned();
    }
    if preview {
        if let Some(stem) = clean.strip_suffix(".wav") {
            clean = format!("{stem}.mp3");
        } else if !clean.to_ascii_lowercase().ends_with(".mp3") {
            clean.push_str(".mp3");
        }
    }
    clean
}

pub(crate) fn catalog_uuid(value: &str) -> Result<(), AppError> {
    let bytes = value.as_bytes();
    let groups = [8, 4, 4, 4, 12];
    let mut offset = 0;
    for (index, length) in groups.iter().enumerate() {
        if index > 0 {
            if bytes.get(offset) != Some(&b'-') {
                return Err(uuid_error());
            }
            offset += 1;
        }
        let end = offset + length;
        if !bytes
            .get(offset..end)
            .is_some_and(|part| part.iter().all(u8::is_ascii_hexdigit))
        {
            return Err(uuid_error());
        }
        offset = end;
    }
    if offset == bytes.len() {
        Ok(())
    } else {
        Err(uuid_error())
    }
}

fn uuid_error() -> AppError {
    AppError::new(
        "uuid_invalid",
        "That is not a Splice catalog UUID.",
        "Copy catalogUuid from spliceclaw search. The file hash is not the download id.",
    )
}

fn sample_hit(item: &Value) -> Option<SampleHit> {
    let file_hash = item.get("uuid").and_then(Value::as_str)?.to_owned();
    let catalog_uuid = item.get("catalog_uuid").and_then(Value::as_str)?.to_owned();
    if catalog_uuid.len() != 36 || file_hash.len() != 64 {
        return None;
    }
    let name = item.get("name").and_then(Value::as_str)?.trim().to_owned();
    if name.is_empty() {
        return None;
    }
    let files = item.get("files").and_then(Value::as_array);
    let (preview_url, _, _) = classify_files(files);
    Some(SampleHit {
        page_url: format!("https://splice.com/sounds/sample/{file_hash}"),
        catalog_uuid,
        file_hash,
        name,
        duration_ms: item.get("duration").and_then(Value::as_u64),
        category: item
            .get("asset_category_slug")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        licensed: item
            .get("licensed")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        credit_cost: credit_cost(item.get("asset_prices")),
        preview_url,
    })
}

fn credit_cost(prices: Option<&Value>) -> Option<u64> {
    let list = prices?.as_array()?;
    list.iter().find_map(|price| {
        let currency = price.get("currency").and_then(Value::as_str)?;
        if !currency.eq_ignore_ascii_case("credits") && !currency.eq_ignore_ascii_case("credit") {
            return None;
        }
        price.get("amount").and_then(|amount| {
            amount
                .as_u64()
                .or_else(|| amount.as_f64().map(|value| value as u64))
        })
    })
}

fn classify_files(files: Option<&Vec<Value>>) -> (Option<String>, Option<String>, Option<String>) {
    let mut preview = None;
    let mut source = None;
    let mut source_kind = None;
    for file in files.into_iter().flatten() {
        let kind = file
            .get("asset_file_type_slug")
            .and_then(Value::as_str)
            .unwrap_or("");
        let url = file.get("url").and_then(Value::as_str).unwrap_or("");
        if url.is_empty() || !https_asset_url(url) {
            continue;
        }
        if kind == "preview_mp3" && preview.is_none() {
            preview = Some(url.to_owned());
        } else if kind != "preview_mp3" && kind != "waveform" && source.is_none() {
            source = Some(url.to_owned());
            source_kind = Some(kind.to_owned());
        }
    }
    (preview, source, source_kind)
}

fn https_asset_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    if parsed.scheme() != "https" || !parsed.username().is_empty() || parsed.password().is_some() {
        return false;
    }
    let Some(host) = parsed.host_str() else {
        return false;
    };
    host == "spliceblob.splice.com"
        || host.ends_with(".amazonaws.com")
        || host.ends_with(".splice.com")
}

fn graphql(session: &Session, query: &str, variables: Value) -> Result<Value, AppError> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let mut authorization = HeaderValue::from_str(&session.authorization()).map_err(|_| {
        AppError::new(
            "session_invalid",
            "The Splice session cannot be sent.",
            "Open splice.com in Firefox and sign in again.",
        )
    })?;
    authorization.set_sensitive(true);
    headers.insert(AUTHORIZATION, authorization);
    let client = Client::builder()
        .timeout(Duration::from_secs(25))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(user_agent())
        .default_headers(headers)
        .build()
        .map_err(|_| unreachable_error())?;
    let response = client
        .post(GRAPHQL)
        .json(&json!({ "query": query, "variables": variables }))
        .send()
        .map_err(|_| unreachable_error())?;
    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(AppError::http(
            "session_rejected",
            "Splice rejected the Firefox session.",
            "Open splice.com in that Firefox profile and sign in again. Do not paste a cookie.",
            status.as_u16(),
        ));
    }
    if !status.is_success() {
        return Err(AppError::http(
            "splice_http",
            "Splice returned an unexpected status.",
            "Stop. The response body is discarded.",
            status.as_u16(),
        ));
    }
    let body: Value = response.json().map_err(|_| unexpected())?;
    if body
        .get("errors")
        .and_then(Value::as_array)
        .is_some_and(|errors| !errors.is_empty())
        && body.get("data").is_none_or(Value::is_null)
    {
        return Err(unexpected());
    }
    Ok(body)
}

fn download_client() -> Result<Client, AppError> {
    Client::builder()
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(3))
        .user_agent(user_agent())
        .build()
        .map_err(|_| unreachable_error())
}

fn unexpected() -> AppError {
    AppError::http(
        "splice_response_unexpected",
        "Splice returned a response this CLI does not understand.",
        "The body was discarded.",
        200,
    )
}

fn unreachable_error() -> AppError {
    AppError::http(
        "splice_unreachable",
        "Splice did not respond.",
        "Check the network. Do not retry automatically.",
        0,
    )
}

fn output_error() -> AppError {
    AppError::new(
        "output_failed",
        "Cannot write the audio file.",
        "Check that the --out directory is writable. No cookie was written.",
    )
}

pub(crate) fn purchase_required() -> AppError {
    AppError::new(
        "purchase_required",
        "This sample is not licensed yet.",
        "Re-run with --confirm to spend 1 Splice credit, or add --preview to use the preview MP3 without spending a credit.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn source_plan_does_not_purchase_without_confirm() {
        assert_eq!(
            plan_source(MediaChoice::Source, false, false, false),
            SourcePlan::NeedConfirm
        );
        assert_eq!(
            plan_source(MediaChoice::Source, false, false, true),
            SourcePlan::PurchaseThenSource
        );
        assert_eq!(
            plan_source(MediaChoice::Preview, false, false, true),
            SourcePlan::Preview
        );
        assert_eq!(
            plan_source(MediaChoice::Source, true, true, false),
            SourcePlan::Source
        );
    }

    #[test]
    fn parses_sample_without_keeping_waveform() {
        let item = json!({
            "uuid": "a".repeat(64),
            "catalog_uuid": "6aee99fa-53a3-4ead-8535-0172bff60fea",
            "name": "Pack/One-Shots/Footstep.wav",
            "duration": 281,
            "asset_category_slug": "oneshot",
            "licensed": false,
            "asset_prices": [{"amount": 1, "currency": "credits"}],
            "files": [
                {"asset_file_type_slug": "preview_mp3", "url": "https://spliceproduction.s3.us-west-1.amazonaws.com/preview.mp3"},
                {"asset_file_type_slug": "waveform", "url": "https://spliceblob.splice.com/wave.json"},
                {"asset_file_type_slug": "source", "url": "https://spliceproduction.s3.us-west-1.amazonaws.com/source.wav"}
            ]
        });
        let hit = sample_hit(&item).unwrap();
        assert_eq!(hit.credit_cost, Some(1));
        assert!(hit.preview_url.unwrap().ends_with("preview.mp3"));
        assert_eq!(output_file_name(&hit.name, false), "Footstep.wav");
        assert_eq!(output_file_name(&hit.name, true), "Footstep.mp3");
        let files = item.get("files").and_then(Value::as_array);
        let (_preview, source, kind) = classify_files(files);
        assert_eq!(kind.as_deref(), Some("source"));
        assert!(source.unwrap().ends_with("source.wav"));
    }

    #[test]
    fn rejects_non_https_and_bad_uuid() {
        assert!(!https_asset_url("http://spliceblob.splice.com/a.wav"));
        assert!(!https_asset_url(
            "https://user:pass@spliceblob.splice.com/a.wav"
        ));
        assert!(!https_asset_url("https://example.com/a.wav"));
        assert!(catalog_uuid("6aee99fa-53a3-4ead-8535-0172bff60fea").is_ok());
        assert!(catalog_uuid(&"a".repeat(64)).is_err());
    }
}
