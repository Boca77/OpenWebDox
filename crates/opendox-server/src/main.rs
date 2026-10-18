//! `opendox-server [--host 127.0.0.1] [--port 5175] [--db PATH] [--memory]`

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use opendox_server::{Hub, HubConfig, MemoryStorage, SqliteStorage, Storage};
use tokio::net::TcpListener;

const USAGE: &str = "usage: opendox-server [--host ADDR] [--port N] [--db PATH | --memory]

  --host ADDR   interface to listen on (default 127.0.0.1; there is no authentication yet)
  --port N      port (default 5175)
  --db PATH     SQLite file (default: opendox/opendox.sqlite3 in the system temp folder)
  --memory      keep documents in memory only";

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
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            _ => return Err(format!("unknown argument {a}\n{USAGE}")),
        }
    }

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
        r = opendox_server::serve(listener, hub) => r.map_err(|e| e.to_string()),
        _ = tokio::signal::ctrl_c() => Ok(()),
    }
}
