use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "spliceclaw",
    version,
    about = "Search your own Splice Sounds samples from the Firefox session"
)]
#[command(
    after_help = "Offline unless --live is set.\nsearch does not spend credits. link prints a URL and does not save a file.\ndownload saves one audio file. An unlicensed source file needs --confirm and spends 1 credit.\n--preview uses the preview MP3 and never purchases.\nCookies are never printed, saved, or accepted as arguments."
)]
pub(crate) struct Cli {
    /// Contact Splice. Without this flag, only local session presence is checked.
    #[arg(long, global = true)]
    pub live: bool,

    /// Firefox profile name from profiles.ini. Not a cookie value.
    #[arg(long, global = true)]
    pub profile: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Check the signed-in Firefox account.
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    /// Report whether Firefox profile configuration exists. Cookies are not opened.
    Doctor,
    /// Search samples. Does not purchase.
    Search {
        /// Search text, for example footstep.
        query: String,
        /// oneshot, loop, or any.
        #[arg(long, default_value = "oneshot")]
        category: String,
        /// Maximum results, 1 through 20.
        #[arg(long, default_value_t = 8)]
        limit: u8,
    },
    /// Print one sample URL. Does not save a file.
    Link {
        /// Catalog UUID from search.
        uuid: String,
        /// Use the preview MP3 and do not purchase.
        #[arg(long)]
        preview: bool,
        /// Spend 1 credit when the source file is not licensed yet.
        #[arg(long)]
        confirm: bool,
    },
    /// Save one sample audio file.
    Download {
        /// Catalog UUID from search.
        uuid: String,
        /// Save the preview MP3 and do not purchase.
        #[arg(long)]
        preview: bool,
        /// Spend 1 credit when the source file is not licensed yet.
        #[arg(long)]
        confirm: bool,
        /// Directory for the saved file.
        #[arg(long, default_value = "splice-downloads")]
        out: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum AuthCommand {
    /// Report session presence, or with --live the username and credits.
    Status,
}

impl Command {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Auth {
                command: AuthCommand::Status,
            } => "auth status",
            Self::Doctor => "doctor",
            Self::Search { .. } => "search",
            Self::Link { .. } => "link",
            Self::Download { .. } => "download",
        }
    }
}
