use std::fs;
use std::path::Path;
use std::{collections::HashMap, sync::Arc};
use clap::Parser;
use log::{debug, error, info, LevelFilter};
use reqwest::Client;
use reqwest::cookie::Jar;
use simple_logger::SimpleLogger;
use anyhow::Result;

const DEFAULT_HOST: &str = "localhost";
const DEFAULT_PORT: &str = "8080";
const DEFAULT_USERNAME: &str = "admin";

#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
struct Args {

    /// QBittorrent WebUI Port
    #[arg(long, default_value_t = String::from(DEFAULT_HOST))]
    ip: String,

    /// QBittorrent WebUI Port
    #[arg(long, default_value_t = String::from(DEFAULT_PORT))]
    port: String,

    /// QBittorrent WebUI Username
    #[arg(long, default_value_t = String::from(DEFAULT_USERNAME))]
    username: String,

    /// QBittorrent WebUI Password
    #[arg(long)]
    password: String,

    /// Level level - All variants given by LogFilter of Log crate.
    #[arg(long, default_value_t = LevelFilter::Info)]
    log_level: LevelFilter,

    /// Delete File Mode
    #[arg(long, value_enum)]
    mv_mode: MvMode,

    /// Place to Move Filtered Files to.
    #[arg(long)]
    mv_directory: String,

    /// Filter tags - Space separated, example: "weird gay porn".
    #[arg(long)]
    tags: Option<String>,

    #[arg(long)]
    category: Option<String>,
    
}

#[derive(Debug, Clone, clap::ValueEnum, PartialEq, PartialOrd)]
enum MvMode {
    None,
    Mv,
    Cp,
}

#[derive(serde::Deserialize, Debug)]
struct TorrentSavePath {
    save_path: String,
}

#[derive(serde::Deserialize, Debug, Clone)]
struct TorrentInfo {
    hash: String,
    category: String,
    tags: String,
    progress: f64,
}

#[derive(serde::Deserialize, Debug)]
struct TorrentFile {
    name: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let _ = SimpleLogger::new().with_level(args.log_level).init();

    let api_url = format!("http://{}:{}/api/v2", args.ip, args.port);

    debug!("QBittorrent API URL: {}", api_url);

    let client = get_login_client(&args, &api_url).await?;

    let save_path: String = client
        .get(format!("{}/app/preferences", api_url))
        .send()
        .await?
        .json()
        .await
        .and_then(|save_path: TorrentSavePath| Ok(save_path.save_path))?;

    debug!("QBittorrent Save Path: {}", save_path);

    let all_torrent_info: Vec<TorrentInfo> = client
        .get(format!("{}/torrents/info", api_url))
        .send()
        .await?
        .json()
        .await?;

    let filtered_torrent_info = get_filtered_torrent_info(&args, &all_torrent_info).await?;

    let filtered_torrent_files = get_filtered_torrent_files(&args, &client, &api_url, &save_path, filtered_torrent_info).await?;

    transfer_files(&args, &filtered_torrent_files).await?;

    Ok(())
}

async fn get_login_client(args: &Args, api_url: &str) -> Result<Client> {
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

async fn get_filtered_torrent_info(args: &Args, all_torrent_info: &Vec<TorrentInfo>) -> Result<Vec<TorrentInfo>> {
    let category_filter = &args.category;
    let filtered_torrent_info = all_torrent_info.iter().filter(|torrent_info| {
        let torrent_tags: Vec<&str> = torrent_info.tags.split(", ").collect();
        torrent_info.progress.eq(&1.0) 
        && (category_filter.is_none() || torrent_info.category.eq(&category_filter.clone().unwrap())) 
        && (args.tags.is_none() || tags_match(args.tags.clone().unwrap().split(", ").collect(), torrent_tags))
    }).cloned().collect();

    Ok(filtered_torrent_info)
}

fn tags_match(filter_tags: Vec<&str>, torrent_tags: Vec<&str>) -> bool {
    filter_tags.iter().all(|filter_tag| torrent_tags.contains(filter_tag))
}

async fn get_filtered_torrent_files(
    args: &Args,
    client: &Client, 
    api_url: &str, 
    save_path: &str, 
    all_torrent_info:Vec<TorrentInfo>, 
) -> Result<HashMap<String, String>> {
    let mut all_torrent_files = HashMap::new();

    for torrent in all_torrent_info.iter() {
        let url = format!("{}/torrents/files?hash={}", api_url, torrent.hash);
    
        let files: Vec<TorrentFile> = client.get(url)
            .send()
            .await?
            .json()
            .await?;

        files.iter().for_each(|file| {
            all_torrent_files.insert(format!("{}/{}", save_path, file.name), format!("{}/{}", args.mv_directory, file.name));
        });
    }

    Ok(all_torrent_files)
}

async fn transfer_files(args: &Args, filtered_torrent_files: &HashMap<String, String>) -> Result<()> {
    filtered_torrent_files.iter().for_each(|(old_file_string_path, new_file_string_path)| {
        let new_file_path = Path::new(new_file_string_path);
        match args.mv_mode {
            MvMode::None => {
                // Do nothing.
            },
            MvMode::Mv => {
                if let Err(err) = fs::rename(old_file_string_path, new_file_string_path) {
                    error!("{}", err,);
                }
            },
            MvMode::Cp => {
                if let Some(parent) = new_file_path.parent() {
                    if let Err(err) = fs::create_dir_all(parent) {
                        error!("Failed to create parent directory: {}", err);
                    }
                }
                if let Err(err) = fs::copy(old_file_string_path, new_file_string_path) {
                    error!("Failed to copy from {} to {}, Error: {}", old_file_string_path, new_file_string_path, err);
                }
            },
        }
    });

    Ok(())
}