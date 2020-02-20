use chrono::DateTime;
use config::*;
use futures::{stream, StreamExt};
use regex::Regex;
use rss::Channel;
use std::error::Error;
use std::fs::File;
use std::fs;
use std::io::ErrorKind;
use std::io::prelude::*;
use std::path::PathBuf;
use std::time;
use tokio;

struct Tfile {
    name: std::path::PathBuf,
    content_type: String,
    content: std::result::Result<bytes::Bytes, reqwest::Error>,
}

pub struct  Rresult {
    pub urls: Vec<String>,
    pub mindate: String,
}

pub struct Conf {
    pub url: String,
    pub path: String,
    pub verbose: bool,
    pub parallel_downloads: usize,
    pub quit: bool,
    pub sleep: time::Duration,
    pub regex_true: regex::Regex,
    pub regex_false: regex::Regex,
}

impl Conf {
    pub fn new(filename: &str) -> Result<Conf, ConfigError> {
        let mut settings = Config::default();

        settings.merge(config::File::with_name(filename))?;
        
        let url = settings.get::<String>("url")?; 

        let verbose = match settings.get::<String>("verbose") {
            Ok(verbose) => match &verbose[..] {
                "true" => true,
                "1" => true,
                "on" => true,
                _ => false,
            }
            Err(_) => true
        };

        let parallel_downloads = settings.get::<usize>("parallel_download").unwrap_or_else(|error| {
            if verbose {
                println!("Warning: {} in config.toml file. Default parallel requests set to 2.", error);
            }
            2
        });
        
        let path = settings.get::<String>("path").unwrap_or_else(|error| {
            if verbose {
                println!("Warning: {} in config.toml file. Current directory will be used.", error);
            }
            String::from("./")
        });

        let quit = match settings.get::<String>("run_forever") {
            Ok(run_forever) => match &run_forever[..] {
                "true" => false,
                "1" => false,
                "on" => false,
                _ => true,
            },
            Err(_) => true
        };

        let sleep = match settings.get::<u64>("check_timer") {
            Ok(ct) => time::Duration::from_secs(ct * 60),
            Err(error) => {
                if verbose {
                    println!("Warning: {} in config.toml file. Default delay between checking RSS feed set to 15 minuts.", error);
                }
                time::Duration::from_secs(900)
            }
        };

        let regex_true = settings.get::<String>("match").unwrap_or_else(|error| {
            if verbose {
                println!("{}", error);
            }
            String::from("")
        });

        let regex_false = settings.get::<String>("match_false").unwrap_or_else(|error| {
            if verbose {
                println!("{}", error);
            }
            String::from("")
        });

        let mut regex_true_string = String::from(r"(?i)^");
        let mut regex_false_string = String::from(r"(?i)^");
        if regex_true.len() > 0 {
            regex_true_string.push_str(&regex_true);
        }
        else {
            regex_true_string.push_str(&".*");
        }
        regex_false_string.push_str(&regex_false);
        regex_true_string.push_str("$");
        regex_false_string.push_str("$");

        let regex_true = Regex::new(&regex_true_string).unwrap();
        let regex_false = Regex::new(&regex_false_string).unwrap();

        Ok(Conf { url, path, verbose, parallel_downloads, quit, sleep, regex_true, regex_false})
    }
}

#[tokio::main]
pub async fn download_files(urls: &[String], config: &Conf) {
    let client = reqwest::Client::new();

    let file_content = stream::iter(urls)
        .map(|url| {
            let client = &client;
            async move {
                let resp = client.get(url).send().await;
                let resp = resp.unwrap();
                let header = resp.headers().get("content-type").unwrap().to_str().unwrap().to_string();
                let filename = match resp.headers().get("content-disposition") {
                    Some(i) => {
                        let tmp = i.to_str().unwrap();
                        &tmp[22..tmp.len()-1]
                    },
                    None => {
                        resp.url()
                            .path_segments()
                            .and_then(|segments| segments.last())
                            .and_then(|name| if name.is_empty() { None } else { Some(name) })
                            .unwrap_or("tmp.torrent")
                    }
                };
                let mut p = PathBuf::from(&config.path);
                p.push(filename);
                let content = resp.bytes().await;
                Tfile {name: p, content_type: header, content: content  }
            }
        })
        .buffer_unordered(config.parallel_downloads);

    file_content
        .for_each(|b| {
            async {
                match &b.content_type[..] {
                    "application/x-bittorrent" => {
                        match b.content {
                            Ok(c) => {
                                let mut dest = File::create(b.name).unwrap();
                                match dest.write_all(&c) {
                                    Ok(_) => (),
                                    Err(e) => if config.verbose { eprintln!("Got an error: {}", e) },
                                }
                            },
                            Err(e) => if config.verbose { eprintln!("Got an error: {}", e) },
                        }
                    }
                    _ => if config.verbose { eprintln!("Rss url does not link to a torrent file.") }
                }
            }
        })
        .await;
}

pub fn write_savedstate(pub_date: String) -> Result<(), std::io::Error> {
    let mut file = File::create("savedstate.dat")?;
    file.write_all(pub_date.as_bytes())?;
    Ok(())
}

pub fn read_savedstate() -> DateTime<chrono::offset::FixedOffset> {
    let content = match fs::read_to_string("savedstate.dat") {
        Ok(s) => s.trim().to_string(),
        Err(error) => match error.kind() {
            ErrorKind::PermissionDenied => {
                println!("Warning: {}. Can't read from savedstate.dat.", error);
                "0000-01-28T07:55:39-05:00".to_string()
            },
            _ => "0000-01-28T07:55:39-05:00".to_string(),
        },
    };
    
    let date = DateTime::parse_from_rfc3339(&content);
    match date {
        Ok(date) => date,
        Err(_) => DateTime::parse_from_rfc3339("0000-01-28T07:55:39-05:00").unwrap(),
    }
}

pub fn get_new_urls(config: &Conf, date: &DateTime<chrono::offset::FixedOffset>) -> Result<Rresult, Box<dyn Error>> {
    let mut content = reqwest::blocking::get(&config.url)?;
    let mut buf: Vec<u8> = vec![];
    content.copy_to(&mut buf)?;
    let channel = Channel::read_from(&buf[..])?;
    let items = channel.items();

    let mut mindate = *date;
    let mut urls: Vec<String> = vec![];
    
    for item in items {        
        let time = DateTime::parse_from_rfc2822(item.pub_date().unwrap()).unwrap();
        if time > *date {
            let item_link = match item.link(){
                Some(i) => i,
                None => {
                    if config.verbose {
                        println!("No link.");
                    }
                    continue
                }, 
            };

            let item_title = match item.title() {
                Some(i) => i,
                None => "",
            };

            let is_match = config.regex_true.is_match(item_title);
            let not_match = !config.regex_false.is_match(item_title);

            if is_match && not_match {
                urls.push(item_link.to_string());
            }

            if time > mindate {
                mindate = time;
            }
        }
    }

    let mindate = mindate.to_rfc3339().to_string();
    
    Ok(Rresult {urls, mindate})
}
