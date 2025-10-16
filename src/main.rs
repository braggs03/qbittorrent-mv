use std::collections::HashSet;
use std::fs;
use std::{collections::HashMap, sync::Arc};

use clap::Parser;
use reqwest::Client;
use reqwest::cookie::Jar;
use walkdir::WalkDir;

const DEFAULT_HOST: &str = "localhost";
const DEFAULT_SSH_PORT: &str = "22";
const DEFAULT_SSH_SAVE_PATH: &str = "EMPTY";

const SSH: &str = "ssh";
const TRUE: &str = "true";

#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
struct Args {

    /// QBittorrent WebUI Port
    #[arg(long)]
    port: String,

    /// QBittorrent WebUI Username
    #[arg(long)]
    username: String,

    /// QBittorrent WebUI Password
    #[arg(long)]
    password: String,

    // Output Mode - Outputs Dangling Files
    #[clap(long, default_value_t = OutputLevel::Info, value_enum)]
    output: OutputLevel,

    /// Delete File Mode
    #[arg(long, default_value_t = false)]
    mv: bool,

    /// Place to Move Filtered Files to.
    #[arg(long)]
    mv_directory: String,

    /// Filter tags - Example: "weird gay porn"
    #[arg(long)]
    tags: String,

    #[arg(long)]
    category: String,
    
}

#[derive(Debug, Clone, clap::ValueEnum, PartialEq, PartialOrd)]
enum OutputLevel {
    None = 0,
    Info = 1,
    Debug = 2,
}

impl OutputLevel {
    fn within(&self, required: OutputLevel) -> bool {
        *self >= required
    }
}

#[derive(serde::Deserialize, Debug)]
struct TorrentSavePath {
    save_path: String,
}

#[derive(serde::Deserialize, Debug)]
struct TorrentInfo {
    hash: String,
}

#[derive(serde::Deserialize, Debug)]
struct TorrentFile {
    name: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let api_url = format!("http://{}:{}/api/v2", DEFAULT_HOST, args.port);

    let client = get_login_client(&args, &api_url).await?;

    let save_path: String = client
        .get(format!("{}/app/preferences", api_url))
        .send()
        .await?
        .json()
        .await
        .and_then(|save_path: TorrentSavePath| Ok(save_path.save_path))?
        .replace("\\", "/");

    let torrent_hashes: Vec<TorrentInfo> = client
        .get(format!("{}/torrents/info", api_url))
        .send()
        .await?
        .json()
        .await?;

    let all_torrent_files = get_torrent_files(&args, &client, &api_url, &save_path, torrent_hashes).await?;

    let filtered_torrent_files = filter_torrent_files(&args, all_torrent_files, &save_path);

    Ok(())
}

async fn get_login_client(args: &Args, api_url: &str) -> Result<Client, Box<dyn std::error::Error>> {
    let mut req_credentials = HashMap::new();
    req_credentials.insert("username", &args.username);
    req_credentials.insert("password", &args.password);

    let cookie_store = Arc::new(Jar::default());
    let client = Client::builder()
        .cookie_provider(cookie_store.clone())
        .build()?;

    let _ = client.post(format!("{}/auth/login", api_url))
        .form(&req_credentials)
        .send()
        .await?;

    Ok(client)
}

async fn get_torrent_files(
    args: &Args,
    client: &Client, 
    api_url: &str, 
    save_path: &str, 
    all_torrent_info:Vec<TorrentInfo>, 
) -> Result<HashSet<String>, Box<dyn std::error::Error>> {

    let mut all_torrent_files = HashSet::new();

    for torrent in all_torrent_info.iter() {
        let url = format!("{}/torrents/files?hash={}", api_url, torrent.hash);
    
        let files: Vec<TorrentFile> = client.get(url)
            .send()
            .await?
            .json()
            .await?;
            
        files.iter().for_each(|file| {
            if args.output.within(OutputLevel::Debug) {
                println!("{}", format!("{}/{}", save_path, file.name).replace("\\", "/"));
            }
            all_torrent_files.insert(format!("{}/{}", save_path, file.name).replace("\\", "/"));
        });
    }

    Ok(all_torrent_files)
}

async fn remove_torrent_files_and_directories(args: &Args, all_torrent_files: HashSet<String>, save_path: &str) -> Result<(), Box<dyn std::error::Error>> {

    let mut entries = Vec::new();

    WalkDir::new(format!("{}", save_path)).into_iter().for_each(
        |entry| {
            entries.push(entry.unwrap());
        }
    );

    entries.sort_by(|a,b| {
        match a.depth().cmp(&b.depth()) {
            std::cmp::Ordering::Less => std::cmp::Ordering::Greater,
            std::cmp::Ordering::Greater => std::cmp::Ordering::Less,
            std::cmp::Ordering::Equal => {
                if a.file_type().is_dir() {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater 
                }
            },
        }
    });

    for entry in entries {
        let file_path = entry.path().to_str().expect("Error: Walk error.");
        let file_path = file_path.replace("\\", "/");
        let file_type = entry.file_type();

        if file_type.is_file() && !all_torrent_files.contains(&file_path) {
            if args.output.within(OutputLevel::Info) {
                println!("Found dangling file: {}", &file_path);
            }
            if args.mv {
                match fs::remove_file(&file_path) {
                    Ok(_) => {},
                    Err(err) => {
                        if args.output.within(OutputLevel::Info) {
                            println!("Removing File Error: {}, {}", err, &file_path)
                        }
                    },
                }
            }
        } else if file_type.is_dir() && fs::read_dir(&file_path).unwrap().next().is_none() && !save_path.eq(&file_path) {
            if args.output.within(OutputLevel::Info) {
                println!("Found dangling folder: {}", &file_path);
            }
            if args.mv {
                match fs::remove_dir(&file_path) {
                    Err(err) => {
                        if args.output.within(OutputLevel::Info) {
                            println!("Removing Empty Directory Error: {}, {}", err, &file_path)
                        }
                    },
                    _ => {},
                }
            }
        }
    }  



    Ok(())
}