//! `opendox-server [--host 127.0.0.1] [--port 5175] [--db PATH | --memory] [--secret S]`
//! `opendox-server --issue DOC view|edit SECONDS` (prints a token; needs the secret)

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use opendox_server::{
    Access, AllowAll, Authorizer, Hub, HubConfig, MemoryStorage, SignedTokens, SqliteStorage,
    Storage,
};
use tokio::net::TcpListener;

const USAGE: &str = "usage: opendox-server [--host ADDR] [--port N] [--db PATH | --memory]
                      [--secret SECRET] [--insecure]
       opendox-server --secret SECRET --issue DOC view|edit SECONDS

  --host ADDR   interface to listen on (default 127.0.0.1)
  --port N      port (default 5175)
  --db PATH     SQLite file (default: opendox/opendox.sqlite3 in the system temp folder)
  --memory      keep documents in memory only
  --secret S    require signed tokens (?token=...) made with this secret; also read from the
                OPENDOX_SECRET environment variable. Use 32+ random characters.
  --insecure    without a secret, let anyone edit anything even on a public interface
  --issue       print a token for DOC (or * for all documents) valid for SECONDS, and exit

Without a secret, everyone may edit every document, so the server only listens on
localhost unless --insecure is given.";

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("opendox-server: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let mut host = "127.0.0.1".to_owned();
    let mut port = 5175u16;
    let mut db: Option<PathBuf> = None;
    let mut memory = false;
    let mut secret = std::env::var("OPENDOX_SECRET")
        .ok()
        .filter(|s| !s.is_empty());
    let mut insecure = false;
    let mut issue: Option<(String, String, String)> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("{a} needs a value\n{USAGE}"))
        };
        match a.as_str() {
            "--host" => host = value()?,
            "--port" => port = value()?.parse().map_err(|_| format!("bad port\n{USAGE}"))?,
            "--db" => db = Some(value()?.into()),
            "--memory" => memory = true,
            "--secret" => secret = Some(value()?),
            "--insecure" => insecure = true,
            "--issue" => issue = Some((value()?, value()?, value()?)),
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            _ => return Err(format!("unknown argument {a}\n{USAGE}")),
        }
    }

    if let Some((doc, access, seconds)) = issue {
        let tokens = SignedTokens::new(secret.ok_or(format!("--issue needs a secret\n{USAGE}"))?);
        let access = match access.as_str() {
            "view" => Access::View,
            "edit" => Access::Edit,
            _ => return Err(format!("access is view or edit\n{USAGE}")),
        };
        let seconds: u64 = seconds
            .parse()
            .map_err(|_| format!("bad SECONDS\n{USAGE}"))?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs();
        println!("{}", tokens.issue(&doc, access, now + seconds));
        return Ok(());
    }
    let auth: Arc<dyn Authorizer> = match &secret {
        Some(s) if s.len() < 16 => {
            return Err("the secret is too short: it needs at least 16 characters (32+ random ones recommended)".into())
        }
        Some(s) => {
            println!("access: signed tokens required");
            Arc::new(SignedTokens::new(s.as_bytes()))
        }
        None => {
            let local = host == "127.0.0.1" || host == "::1" || host == "localhost";
            if !local && !insecure {
                return Err(format!(
                    "refusing to listen on {host} with no access control: pass --secret (or \
                     --insecure to let anyone edit anything)"
                ));
            }
            println!("access: anyone may edit any document (no --secret)");
            Arc::new(AllowAll)
        }
    };

    let storage: Arc<dyn Storage> = if memory {
        println!("storage: in memory (lost on exit)");
        Arc::new(MemoryStorage::new())
    } else {
        let path =
            db.unwrap_or_else(|| std::env::temp_dir().join("opendox").join("opendox.sqlite3"));
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        println!("storage: {}", path.display());
        Arc::new(SqliteStorage::open(&path).map_err(|e| e.to_string())?)
    };

    let hub = Arc::new(Hub::new(storage, HubConfig::default()));
    let listener = TcpListener::bind((host.as_str(), port))
        .await
        .map_err(|e| format!("cannot listen on {host}:{port}: {e}"))?;
    println!("opendox sync server: ws://{host}:{port}/doc/<id>");
    tokio::select! {
        r = opendox_server::serve_with(listener, hub, auth) => r.map_err(|e| e.to_string()),
        _ = tokio::signal::ctrl_c() => Ok(()),
    }
}
