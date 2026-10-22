mod cli;
mod error;
mod firefox;
mod splice;

use std::io::{self, Write};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use serde_json::{Value, json};

use crate::cli::{Cli, Command};
use crate::error::AppError;
use crate::firefox::{
    Locations, env_token_present, firefox_executable_present, firefox_present, resolve_session,
};
use crate::splice::{
    MediaChoice, SourcePlan, catalog_uuid, download_file, fetch_account, fetch_asset,
    output_file_name, plan_source, purchase, purchase_required, search,
};

fn success(command: &str, summary: &str, data: Value) -> Value {
    json!({
        "ok": true,
        "command": command,
        "cliVersion": env!("CARGO_PKG_VERSION"),
        "summary": summary,
        "data": data
    })
}

fn failure(command: &str, error: &AppError) -> Value {
    json!({
        "ok": false,
        "command": command,
        "cliVersion": env!("CARGO_PKG_VERSION"),
        "summary": error.message,
        "error": error,
        "hints": [error.hint]
    })
}

fn now_seconds() -> Result<u64, AppError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| {
            AppError::new(
                "clock_invalid",
                "The current time is outside the supported range.",
                "Check the OS clock.",
            )
        })
}

fn require_live(live: bool) -> Result<(), AppError> {
    if live {
        Ok(())
    } else {
        Err(AppError::new(
            "live_required",
            "This command contacts Splice only when --live is set.",
            "Add --live. Cookies are still not printed.",
        ))
    }
}

fn category(value: &str) -> Result<Option<&str>, AppError> {
    match value {
        "oneshot" | "loop" => Ok(Some(value)),
        "any" => Ok(None),
        _ => Err(AppError::new(
            "category_invalid",
            "The sample category must be oneshot, loop, or any.",
            "Use --category oneshot for sound effects.",
        )),
    }
}

fn search_query(value: &str) -> Result<&str, AppError> {
    let query = value.trim();
    if query.is_empty() || query.len() > 80 || query.bytes().any(|byte| byte < 0x20 || byte == 0x7f)
    {
        return Err(AppError::new(
            "query_invalid",
            "The search text is empty or too long.",
            "Use a short term such as footstep.",
        ));
    }
    Ok(query)
}

fn run(cli: &Cli) -> Result<Value, AppError> {
    let command = cli.command.label();
    let locations = Locations::discover()?;
    match &cli.command {
        Command::Doctor => Ok(success(
            command,
            "Local transport presence only. Cookie values were not opened.",
            json!({
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "firefoxConfig": firefox_present(&locations.firefox),
                "firefoxExecutable": firefox_executable_present(),
                "envToken": env_token_present()
            }),
        )),
        Command::Auth { .. } => {
            let session =
                resolve_session(&locations.firefox, cli.profile.as_deref(), now_seconds()?)?;
            if !cli.live {
                return Ok(success(
                    command,
                    "Splice session is present. Splice was not contacted.",
                    json!({
                        "firefoxProfile": session.profile,
                        "live": false,
                        "session": "present"
                    }),
                ));
            }
            let account = fetch_account(&session)?;
            Ok(success(
                command,
                "Read the Splice account with one request. No sample was purchased.",
                json!({
                    "firefoxProfile": session.profile,
                    "live": true,
                    "account": account
                }),
            ))
        }
        Command::Search {
            query,
            category: category_name,
            limit,
        } => {
            require_live(cli.live)?;
            if !(1..=20).contains(limit) {
                return Err(AppError::new(
                    "limit_invalid",
                    "The result limit must be from 1 to 20.",
                    "Run search again with --limit 8.",
                ));
            }
            let session =
                resolve_session(&locations.firefox, cli.profile.as_deref(), now_seconds()?)?;
            let (records, hits) = search(
                &session,
                search_query(query)?,
                category(category_name)?,
                *limit,
            )?;
            Ok(success(
                command,
                "Searched samples. No credit was spent.",
                json!({
                    "firefoxProfile": session.profile,
                    "records": records,
                    "category": category_name,
                    "samples": hits
                }),
            ))
        }
        Command::Link {
            uuid,
            preview,
            confirm,
        } => {
            require_live(cli.live)?;
            catalog_uuid(uuid)?;
            let session =
                resolve_session(&locations.firefox, cli.profile.as_deref(), now_seconds()?)?;
            let asset = resolve_asset(&session, uuid, *preview, *confirm)?;
            Ok(success(
                command,
                "Returned one sample URL. The file was not saved.",
                asset,
            ))
        }
        Command::Download {
            uuid,
            preview,
            confirm,
            out,
        } => {
            require_live(cli.live)?;
            catalog_uuid(uuid)?;
            let session =
                resolve_session(&locations.firefox, cli.profile.as_deref(), now_seconds()?)?;
            let asset = resolve_asset(&session, uuid, *preview, *confirm)?;
            let url = asset
                .get("url")
                .and_then(Value::as_str)
                .ok_or_else(purchase_required)?;
            let name = asset
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("sample");
            let file_name = output_file_name(name, *preview);
            let path = download_file(url, out, &file_name)?;
            Ok(success(
                command,
                "Saved one audio file.",
                json!({
                    "name": name,
                    "kind": asset.get("kind"),
                    "purchased": asset.get("purchased"),
                    "path": path
                }),
            ))
        }
    }
}

fn resolve_asset(
    session: &crate::firefox::Session,
    uuid: &str,
    preview: bool,
    confirm: bool,
) -> Result<Value, AppError> {
    let mut asset = fetch_asset(session, uuid)?;
    let choice = if preview {
        MediaChoice::Preview
    } else {
        MediaChoice::Source
    };
    let plan = plan_source(choice, asset.licensed, asset.source_url.is_some(), confirm);
    let purchased = match plan {
        SourcePlan::NeedConfirm => return Err(purchase_required()),
        SourcePlan::PurchaseThenSource => {
            purchase(session, uuid)?;
            asset = fetch_asset(session, uuid)?;
            true
        }
        SourcePlan::Preview | SourcePlan::Source => false,
    };
    let (kind, url) = match plan {
        SourcePlan::Preview => ("preview_mp3", asset.preview_url),
        SourcePlan::Source | SourcePlan::PurchaseThenSource => (
            asset.source_kind.as_deref().unwrap_or("source"),
            asset.source_url,
        ),
        SourcePlan::NeedConfirm => unreachable!(),
    };
    let Some(url) = url else {
        return Err(AppError::new(
            "media_missing",
            "Splice did not return a usable audio URL.",
            "Try --preview, or --confirm if the source file is not licensed yet.",
        ));
    };
    Ok(json!({
        "name": asset.name,
        "licensed": asset.licensed,
        "purchased": purchased,
        "kind": kind,
        "url": url
    }))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let command = cli.command.label();
    match run(&cli) {
        Ok(value) => {
            emit(&value);
            ExitCode::SUCCESS
        }
        Err(error) => {
            let code = error.exit_code;
            emit(&failure(command, &error));
            ExitCode::from(code)
        }
    }
}

fn emit(value: &Value) {
    let mut stdout = io::stdout().lock();
    let _ = serde_json::to_writer_pretty(&mut stdout, value);
    let _ = writeln!(stdout);
}
