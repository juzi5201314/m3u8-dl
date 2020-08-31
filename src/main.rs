use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::{Cursor, Read, Stdout};
use std::option::Option::Some;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use aesstream::AesReader;
use colored::Colorize;
use ffcli::FFmpegDefaultArgs;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use log::Level;
use m3u8_rs::playlist::{Key as m3u8Key, MediaPlaylist, Playlist};
use reqwest::StatusCode;
use tokio::prelude::io::AsyncWriteExt;
use tokio::sync::Semaphore;
use url::{ParseError, Url};

use crate::args::Args;

mod args;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Args = argh::from_env();

    if args.transcode {
        let ffmpeg = std::env::var("FFMPEG_PATH").ok().or_else(find_ffmpeg_with_path).expect("Cannot find ffmpeg executable file. Please set the FFMPEG environment variable.");
        if !Path::new(&ffmpeg).exists() {
            panic!(format!("Cannot find: {}", ffmpeg))
        }
    }

    let url = &args.url;
    let content = if url.starts_with("http") {
        download(&Url::parse(url)?).await?
    } else {
        todo!("from local file")
        //read_to_string(url)?.as_bytes().to_vec()
    };

    match m3u8_rs::parse_playlist_res(&content).expect("Invalid .m3u8 format") {
        Playlist::MasterPlaylist(master) => {
            for (i, vs) in master.variants.iter().enumerate() {
                println!("{index}: \n {stream:#?}", index = format!("#{}", i).blue(), stream = vs)
            }
            println!("{}", "Choose a number:".green());

            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;

            let number = input.trim().trim_start_matches("#").parse::<usize>().expect(&"Please select a valid number!".red());
            let stream = master.variants.get(number).expect(&"Please select a valid number!".red());

            let mut url = Url::parse(url)?;
            url.set_path(&stream.uri);

            let play_list = m3u8_rs::parse_playlist_res(&download(&url).await?).expect("play list not found");
            match play_list {
                Playlist::MediaPlaylist(media_list) => parse_media_list(media_list, &args).await?,
                _ => panic!("media play list not found")
            }
        }
        Playlist::MediaPlaylist(list) => {
            parse_media_list(list, &args).await?;
        }
    }

    Ok(())
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum KeyType {
    None,
    Aes128,
    SampleAes
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct Key {
    ty: KeyType,
    iv: Option<String>,
    content: Vec<u8>
}

impl Default for Key {
    fn default() -> Self {
        Key {
            ty: KeyType::None,
            iv: None,
            content: vec![]
        }
    }
}

impl Key {
    pub async fn from_key(k: &m3u8Key) -> anyhow::Result<Self> {
        Ok(match k.method.as_str() {
            "NONE" => {
                Key {
                    ty: KeyType::None,
                    iv: k.iv.clone(),
                    content: vec![]
                }
            },
            "AES-128" => {
                Key {
                    ty: KeyType::Aes128,
                    iv: k.iv.clone(),
                    content: download(&Url::parse(k.uri.as_ref().unwrap())?).await?
                }
            }
            _ => panic!(format!("Unsupported key method: {}", &k.method))
        })
    }

    pub fn decode(&self, data: &[u8]) -> Vec<u8> {
        let decryptor = crypto::aessafe::AesSafe128Decryptor::new(&self.content);
        let mut reader = AesReader::new(Cursor::new(data), decryptor).unwrap();
        let mut v = Vec::new();
        reader.read_to_end(&mut v).unwrap();
        v
    }
}

pub async fn parse_media_list(play_list: MediaPlaylist, args: &Args) -> anyhow::Result<()> {
    let mut key = Arc::new(Key::default());
    let count = play_list.segments.len();
    let pb =  Arc::new(Mutex::new({
        let mut pb = pbr::ProgressBar::new(count as u64);
        pb.show_speed = false;
        pb.show_time_left = false;
        pb
    }));
    let files = Arc::new(Mutex::new(HashMap::with_capacity(count)));

    let dir = Path::new(&args.cache_dir).join("m3u8-dl").join({
        let mut hasher = DefaultHasher::default();
        args.url.hash(&mut hasher);
        hasher.finish().to_string()
    });
    println!("Cache in {}", &dir.to_str().unwrap());
    mkdir(&dir);

    let mut tasks = FuturesUnordered::new();
    let semaphore = Arc::new(Semaphore::new(args.limit));

    for (i, segment) in play_list.segments.iter().enumerate() {
        if let Some(num) = args.num {
            if i >= num {
                println!("max fragment {}", i);
                break
            }
        }

        if let Some(k) = &segment.key {
            key = Arc::new(Key::from_key(k).await?)
        }

        let file_name = dir.join(Path::new(&segment.uri).file_name().unwrap());
        if !args.reload && file_name.is_file() {
            if File::open(&file_name)?.metadata()?.len() != 0 {
                println!("Pass {}", &file_name.file_name().unwrap().to_str().unwrap());
                pb.lock().unwrap().inc();
                files.lock().unwrap().insert(i, file_name);
                continue
            }
        }

        let url = parse_url(&Url::parse(&args.url)?, &segment.uri)?;
        let key = key.clone();
        let files = files.clone();
        let semaphore = semaphore.clone();
        let pb = pb.clone();

        tasks.push(tokio::task::spawn(async move {
            async fn func(
                url: Url,
                key: Arc<Key>,
                files: Arc<Mutex<HashMap<usize, PathBuf>>>,
                semaphore: Arc<Semaphore>,
                i: usize,
                file_name: PathBuf,
                pb: Arc<Mutex<pbr::ProgressBar<Stdout>>>
            ) -> anyhow::Result<()> {
                #[allow(unused_variables)]
                let p = semaphore.acquire().await;

                //println!("Start #{}", i + 1);

                let mut data = download(&url).await?;

                if !matches!(&key.ty, KeyType::None) {
                    data = key.decode(&mut data);
                }

                let mut file = tokio::fs::File::create(&file_name).await?;
                file.write_all(&data).await?;
                //file.sync_data().await.unwrap();
                println!("Completed #{}", i + 1);
                files.lock().unwrap().insert(i, file_name);
                pb.lock().unwrap().inc();

                Ok(())
            };
            func(url, key, files, semaphore, i, file_name, pb).await
        }));
    }

    while let Some(res) = tasks.next().await {
        res??;
    }
    //futures::future::join_all(tasks).await;
    pb.lock().unwrap().finish();
    println!("done! Merging...");

    let output = format!("{}.{}", args.output.as_ref().unwrap_or(&"output".to_owned()), "ts");

    if let Some(p) = Path::new(&output).parent() {
        mkdir(p)
    }

    let mut file = tokio::fs::File::create(Path::new(&output)).await?;
    let files = files.lock().unwrap();
    let mut v = files.keys().collect::<Vec<&usize>>();
    v.sort();
    for i in v {
        let path = files.get(&i).unwrap();
        file.write_all(&tokio::fs::read(path).await?).await?
    }
    file.sync_data().await?;

    println!("Merger completed.");

    if args.transcode {
        transcoding_with_ffmpeg(&output)
    }

    Ok(())
}

pub fn parse_url(url: &Url, uri: &str) -> Result<Url, ParseError> {
    if uri.starts_with("http://") || uri.starts_with("https://") {
        Url::parse(uri)
    } else if uri.starts_with("/") {
        let mut url = url.clone();
        url.set_path(uri);
        Ok(url)
    } else {
        let mut url = url.clone();
        url.set_path(Path::new(url.path()).parent().unwrap().join(uri).to_str().unwrap());
        Ok(url)
    }
}

pub fn find_ffmpeg_with_path() -> Option<String> {
    Some(std::env::var("PATH").ok()?.split(
        if cfg!(target_os = "windows") { ';' } else { ':' }
    ).find(|s| {
        Path::new(s).join(if cfg!(target_os = "windows") { "ffmpeg.exe" } else { "ffmpeg" }).exists()
    })?.to_owned())
}

pub fn transcoding_with_ffmpeg(output_file: &str) {
    let ffmpeg_bin = std::env::var("FFMPEG_PATH").unwrap();

    println!("Transcoding to mp4 with ffmpeg...");

    let args = ffcli::FFmpegArgs::new(Level::Info)
        .default_args(Some(FFmpegDefaultArgs::Quiet))
        .i(output_file)
        .c("copy", "")
        .raw(format!("{}.mp4", output_file))
        .build();
    Command::new(ffmpeg_bin).args(args).status().expect("ffmpeg failed to execute");
}

pub fn mkdir<P: AsRef<Path>>(path: P) {
    std::fs::create_dir_all(&path).expect(&format!("Cannot create directory {}", path.as_ref().to_str().unwrap()))
}

pub async fn download(url: &Url) -> anyhow::Result<Vec<u8>> {
    let resp = reqwest::get(url.as_str()).await?;
    if resp.status() != StatusCode::OK {
        panic!(format!("{} download failed. http code: {}", url.as_str(), resp.status()))
    }
    Ok(resp.bytes().await?.to_vec())
}