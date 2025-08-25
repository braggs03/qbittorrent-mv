use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::{collections::HashMap, sync::Arc};

use clap::Parser;
use reqwest::Client;
use reqwest::cookie::Jar;
use walkdir::WalkDir;

const DEFAULT_HOST: &str = "localhost";
const DEFAULT_CATEGORY: &str = "";
const DEFAULT_TAGS: &str = "";

#[derive(Parser, Debug, Clone)]
#[command(version, about, long_about = None)]
struct Args {

    /// QBittorrent WebUI Port
    #[arg(long)]
    port: String,

    /// QBittorrent WebUI Port
    #[arg(long, default_value_t = String::from(DEFAULT_HOST))]
    ip: String,

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
    #[arg(long, default_value_t = MvMode::None, value_enum)]
    mv_mode: MvMode,

    /// Place to Move Filtered Files to.
    #[arg(long)]
    mv_directory: String,

    /// Filter tags - Example: "weird gay porn"
    #[arg(long, default_value_t = String::from(DEFAULT_TAGS))]
    tags: String,

    #[arg(long, default_value_t = String::from(DEFAULT_CATEGORY))]
    category: String,
    
}

#[derive(Debug, Clone, clap::ValueEnum, PartialEq, PartialOrd)]
enum OutputLevel {
    None = 0,
    Info = 1,
    Debug = 2,
}

#[derive(Debug, Clone, clap::ValueEnum, PartialEq, PartialOrd)]
enum MvMode {
    None,
    Mv,
    Cp,
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
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let api_url = format!("http://{}:{}/api/v2", args.ip, args.port);

    let client = get_login_client(&args, &api_url).await?;

    let save_path: String = client
        .get(format!("{}/app/preferences", api_url))
        .send()
        .await?
        .json()
        .await
        .and_then(|save_path: TorrentSavePath| Ok(save_path.save_path))?
        .replace("\\", "/");

    let all_torrent_info: Vec<TorrentInfo> = client
        .get(format!("{}/torrents/info", api_url))
        .send()
        .await?
        .json()
        .await?;

    let filtered_torrent_info = get_filtered_torrent_info(&args, &all_torrent_info).await?;

    let filtered_torrent_files = get_filtered_torrent_files(&args, &client, &api_url, &save_path, filtered_torrent_info).await?;

    transfer_files(&args, &save_path, &filtered_torrent_files).await?;

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

async fn get_filtered_torrent_info(args: &Args, all_torrent_info: &Vec<TorrentInfo>) -> Result<Vec<TorrentInfo>, Box<dyn std::error::Error>> {
    let category_filter = &args.category;
    let tags_filter: Vec<&str> = args.tags.split(", ").collect();
    let filtered_torrent_info = all_torrent_info.iter().filter(|torrent_info| {
        let torrent_tags: Vec<&str> = torrent_info.tags.split(", ").collect();
        torrent_info.progress.eq(&1.0) && (category_filter.eq(DEFAULT_CATEGORY) || torrent_info.category.eq(category_filter)) && (args.tags.eq(DEFAULT_TAGS) || tags_match(tags_filter.clone(), torrent_tags))
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
) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {

    let mut all_torrent_files = HashMap::new();

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
            all_torrent_files.insert(format!("{}/{}", save_path, file.name).replace("\\", "/"), format!("{}/{}", args.mv_directory, file.name).replace("\\", "/"));
        });
    }

    Ok(all_torrent_files)
}

async fn transfer_files(args: &Args, save_path: &str, filtered_torrent_files: &HashMap<String, String>) -> Result<(), Box<dyn std::error::Error>> {

    filtered_torrent_files.iter().for_each(|(old_file_string_path, new_file_string_path)| {
        let new_file_path = Path::new(new_file_string_path);
        match args.mv_mode {
            MvMode::None => todo!(),
            MvMode::Mv => {
                if let Err(err) = fs::rename(old_file_string_path, new_file_string_path) {
                    println!("{}", err,);
                }
            },
            MvMode::Cp => {
                if let Some(parent) = new_file_path.parent() {
                    if let Err(err) = fs::create_dir_all(parent) {
                        println!("Error creating file parent directory: {}", err);
                    }
                }
                if let Err(err) = fs::copy(old_file_string_path, new_file_string_path) {
                    println!("Error Copying from {} to {}, Error: {}", old_file_string_path, new_file_string_path, err);
                }
            },
        }
    });

    Ok(())
}