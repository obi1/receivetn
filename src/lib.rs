// This file is part of receivetn.
// Copyright (C) 2020  Tadej Obrstar
//
// receivetn is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// receivetn is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with Foobar.  If not, see <https://www.gnu.org/licenses/>.

use chrono::DateTime;
use config::*;
use futures::{stream, StreamExt};
use regex::Regex;
use rss::Channel;
use std::error::Error;
use std::fs::OpenOptions;
use std::fs::File;
use std::fs;
use std::io::ErrorKind;
use std::io::prelude::*;
use std::path::PathBuf;
use std::time;
use tokio;
use structopt::StructOpt;

#[derive(StructOpt)]
pub struct Opt {
    #[structopt(short = "v", long = "verbose", help = "Run in verbose mode")]
    pub verbose: bool,

    #[structopt(parse(from_os_str), short = "c", long = "config", default_value = "config.toml")]
    pub config_file: PathBuf,
}

struct Tfile {
    filename: Filename,
    content: std::result::Result<bytes::Bytes, reqwest::Error>,
}

struct Filename {
    name: String,
    ext: Option<String>,
    path: String,
}

pub struct  Rresult {
    pub urls: Vec<String>,
    pub mindate: String,
}

pub struct Conf {
    pub name: String,
    pub url: String,
    pub path: String,
    pub verbose: bool,
    pub parallel_downloads: usize,
    pub quit: bool,
    pub sleep: time::Duration,
    pub regex_true: regex::Regex,
    pub regex_false: regex::Regex,
}

impl Opt {
    pub fn get() -> Opt {
        Opt::from_args()
    }
}

impl Conf {
    pub fn from_file(opt: &Opt) -> Result<Vec<Conf>, ConfigError> {
        let mut configs: Vec<Conf> = vec![];

        let mut settings = Config::default();

        settings.merge(config::File::with_name(&opt.config_file.to_str().unwrap()))?;

        let active = settings.get::<Vec<String>>("global.enable")?;

        let verbose = if opt.verbose == true {
            opt.verbose
        }
        else {
            match settings.get::<String>("global.verbose") {
                Ok(verbose) => match &verbose[..] {
                    "true" => true,
                    "1" => true,
                    "on" => true,
                    _ => false,
                }
                Err(_) => true
            }
        };

        let parallel_downloads = settings.get::<usize>("global.parallel_download").unwrap_or_else(|error| {
            if verbose {
                println!("Warning: {} in {:?} file. Default parallel requests set to 2.", error, opt.config_file);
            }
            2
        });
        
        let path = settings.get::<String>("global.path").unwrap_or_else(|error| {
            if verbose {
                println!("Warning: {} in {:?} file. Current directory will be used.", error, opt.config_file);
            }
            String::from("./")
        });

        let quit = match settings.get::<String>("global.run_forever") {
            Ok(run_forever) => match &run_forever[..] {
                "true" => false,
                "1" => false,
                "on" => false,
                _ => true,
            },
            Err(_) => true
        };

        let sleep = match settings.get::<u64>("global.check_timer") {
            Ok(ct) => time::Duration::from_secs(ct * 60),
            Err(error) => {
                if verbose {
                    println!("Warning: {} in {:?} file. Default delay between checking RSS feed set to 15 minuts.", error, opt.config_file);
                }
                time::Duration::from_secs(900)
            }
        };

        let regex_true = settings.get::<String>("global.match").unwrap_or_else(|error| {
            if verbose {
                println!("{}", error);
            }
            String::from("")
        });

        let regex_false = settings.get::<String>("global.match_false").unwrap_or_else(|error| {
            if verbose {
                println!("{}", error);
            }
            String::from("")
        });

        for profile in &active {
            let mut url = profile.clone();
            url.push_str(".url");
            let url = match settings.get::<String>(&url[..]) {
                Ok(x) => x,
                Err(_) => continue,
            };

            let verbose_new = if opt.verbose == true {
                opt.verbose
            }
            else {
                let mut verbose_new = profile.clone();
                verbose_new.push_str(".verbose");
                match settings.get::<String>(&verbose_new[..]) {
                    Ok(x) => match &x[..] {
                        "true" => true,
                        "1" => true,
                        "on" => true,
                        _ => false,
                    },
                    Err(_) => verbose,
                }
            };
            
            let mut parallel_downloads_new = profile.clone();
            parallel_downloads_new.push_str(".parallel_download");
            let parallel_downloads_new = settings.get::<usize>(&parallel_downloads_new[..]).unwrap_or(parallel_downloads);
            
            let mut path_new = profile.clone();
            path_new.push_str(".path");
            let path_new = settings.get::<String>(&path_new[..]).unwrap_or(path.clone());
    
            let mut quit_new = profile.clone();
            quit_new.push_str(".run_forever");
            let quit_new = match settings.get::<String>(&quit_new[..]) {
                Ok(x) => match &x[..] {
                    "true" => false,
                    "1" => false,
                    "on" => false,
                    _ => true,
                },
                Err(_) => quit
            };
    
            let mut sleep_new = profile.clone();
            sleep_new.push_str(".check_timer");
            let sleep_new = match settings.get::<u64>(&sleep_new[..]) {
                Ok(ct) => time::Duration::from_secs(ct * 60),
                Err(_) => sleep
            };
    
            let mut regex_true_new = profile.clone();
            regex_true_new.push_str(".match");
            let regex_true_new = settings.get::<String>(&regex_true_new[..]).unwrap_or(regex_true.clone());
    
            let mut regex_false_new = profile.clone();
            regex_false_new.push_str(".match_false");
            let regex_false_new = settings.get::<String>(&regex_false_new[..]).unwrap_or(regex_false.clone());

            let mut regex_true_string = String::from(r"(?i)^");
            let mut regex_false_string = String::from(r"(?i)^");
            if regex_true_new.len() > 0 {
                regex_true_string.push_str(&regex_true_new);
            }
            else {
                regex_true_string.push_str(&".*");
            }
            regex_false_string.push_str(&regex_false_new);
            regex_true_string.push_str("$");
            regex_false_string.push_str("$");
    
            let regex_true_new = Regex::new(&regex_true_string).unwrap();
            let regex_false_new = Regex::new(&regex_false_string).unwrap();

            configs.push(Conf { name: profile.clone(), url: url, path: path_new, verbose: verbose_new, parallel_downloads: parallel_downloads_new, quit: quit_new, sleep: sleep_new, regex_true: regex_true_new, regex_false: regex_false_new})
        }

        Ok(configs)
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
                //let header = resp.headers().get("content-type").unwrap().to_str().unwrap().to_string();
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
                            .unwrap_or("unknown")
                    }
                };
                let path = config.path.to_string();
                let split = filename.rfind(".");
                let name = match split {
                    Some(x) => {
                        filename[..x].to_string()
                    }
                    None => filename.to_string()
                };
                let ext = match split {
                    Some(x) => {
                        Some(filename[x..].to_string())
                    }
                    None => None
                };
                let content = resp.bytes().await;

                Tfile {filename: Filename { name, ext, path}, content: content  }
            }
        })
        .buffer_unordered(config.parallel_downloads);

    file_content
        .for_each(|b| {
            async {
                match b.content {
                    Ok(c) => {
                        let mut dest = find_valid_filename(&b.filename, 0).unwrap();
                        match dest.write_all(&c) {
                            Ok(_) => (),
                            Err(e) => if config.verbose { eprintln!("Got an error: {}", e) },
                        }
                    },
                    Err(e) => if config.verbose { eprintln!("Got an error: {}", e) },
                }
            }
        })
        .await;
}

fn find_valid_filename(filename: &Filename, failed_count: u32) -> Result<File, std::io::Error> {
    let mut path = PathBuf::from(&filename.path);
    let mut file = filename.name.clone();
    
    if failed_count > 0 {
        file.push_str(" (");
        file.push_str(&failed_count.to_string());
        file.push(')');
    }

    match &filename.ext {
        Some(x) => file.push_str(&x),
        None => {},
    }

    path.push(&file);

    match OpenOptions::new().create_new(true).write(true).open(path) {
        Ok(x) => Ok(x),
        Err(error) => match error.kind() {
            ErrorKind::AlreadyExists => {
                let failed_count = failed_count + 1;
                find_valid_filename(&filename, failed_count)
            },
            _ => Err(error),
        }
    }
}

pub fn write_savedstate(name: &str, pub_date: &str) -> Result<(), std::io::Error> {
    let mut file_name = String::from("state_");
    file_name.push_str(name);
    file_name.push_str(".dat");
    let mut file = File::create(file_name)?;
    file.write_all(pub_date.as_bytes())?;
    Ok(())
}

pub fn read_savedstate(name: &str, verbose: &bool) -> DateTime<chrono::offset::FixedOffset> {
    let mut file_name = String::from("state_");
    file_name.push_str(name);
    file_name.push_str(".dat");
    let content = match fs::read_to_string(&file_name) {
        Ok(s) => s.trim().to_string(),
        Err(error) => match error.kind() {
            ErrorKind::PermissionDenied => {
                if *verbose {
                    println!("Warning: {}. Can't read from file {}.", error, &file_name);
                }
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

#[tokio::main]
pub async fn get_new_urls(config: &Conf, date: &DateTime<chrono::offset::FixedOffset>) -> Result<Rresult, Box<dyn Error>> {
    let content: Vec<u8> = reqwest::get(&config.url).await?.bytes().await?.to_vec();
    let channel = Channel::read_from(&content[..])?;
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
