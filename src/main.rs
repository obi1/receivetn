use chrono::DateTime;
use config::*;
use futures::{stream, StreamExt};
use regex::Regex;
use rss::Channel;
use std::fs::File;
use std::io::prelude::*;
use std::path::PathBuf;
use std::{thread, time};
use tokio;

fn main() {
    let mut settings = Config::default();

    match settings.merge(config::File::with_name("config.toml")) {
        Ok(_) => (),
        Err(e) => {
            eprintln!("Error in config file: {}", e);
            return ()
        },
    };
    
    let url = match settings.get::<String>("url") {
        Ok(link) => link,
        Err(error) => {
            println!("Error: {} in config.toml file.", error);
            return ()
        },
    };

    let verbose = match settings.get::<String>("verbose") {
        Ok(verbose) => match &verbose[..] {
            "true" => true,
            "1" => true,
            "on" => true,
            _ => false,
        }
        Err(_) => true
    };

    let parallel_downloads = match settings.get::<usize>("parallel_download") {
        Ok(pd) => pd,
        Err(error) => {
            if verbose {
                println!("Warning: {} in config.toml file. Default parallel requests set to 2.", error);
            }
            2
        },
    };
    
    let path = match settings.get::<String>("path") {
        Ok(path) => path,
        Err(error) => {
            if verbose {
                println!("Warning: {} in config.toml file. Current directory will be used.", error);
            }
            "./".to_string()
        },
    };

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
            println!("Warning: {} in config.toml file. Default delay between checking RSS feed set to 15 minuts.", error);
            time::Duration::from_secs(900)
        }
    };

    let regex_true = match settings.get::<String>("match") {
        Ok(m) => m,
        Err(error) => {
            if verbose {
                println!("{}", error);
            }
            String::from("")
        }
    };

    let regex_false = match settings.get::<String>("match_false") {
        Ok(m) => m,
        Err(error) => {
            if verbose {
                println!("{}", error);
            }
            String::from("")
        }
    };

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

    loop {
        let mut content = match reqwest::blocking::get(url.as_str()) {
            Ok(rssfeed) => rssfeed,
            Err(error) => {
                if verbose { 
                    println!("{}", error);
                }
                if quit {
                    return ()
                }
                else {
                    continue
                }
            },
        };

        let mut buf: Vec<u8> = vec![];
        match content.copy_to(&mut buf) {
            Ok(_) => (),
            Err(error) => {
                if verbose {
                    println!("Error: {}", error);
                }
                if quit {
                    return ()
                }
                else {
                    continue
                }
            },
        };

        let channel = match Channel::read_from(&buf[..]) {
            Ok(data) => data,
            Err(error) => {
                if verbose {
                    println!("Error reading rss feed: {}", error);
                }
                if quit {
                    return ()
                }
                else {
                    continue
                }
            },
        };

        let items = channel.items();
        let sstate = read_savedstate();
        let sstate = match sstate {
            Ok(saved) => saved,
            Err(_) => "0000-01-28T07:55:39-05:00".to_string(),
        };
        let date = DateTime::parse_from_rfc3339(&sstate);
        let date = match date {
            Ok(date) => date,
            Err(_) => DateTime::parse_from_rfc3339("0000-01-28T07:55:39-05:00").unwrap(),
        };
        let mut mindate = date;
        let mut urls: Vec<String> = vec![];
        
        for item in items {        
            let time = DateTime::parse_from_rfc2822(item.pub_date().unwrap()).unwrap();
            if time > date {
                let item_link = match item.link(){
                    Some(i) => i,
                    None => {
                        if verbose {
                            println!("No link.");
                        }
                        continue
                    }, 
                };

                let item_title = match item.title() {
                    Some(i) => i,
                    None => "",
                };

                let is_match = regex_true.is_match(item_title);
                let not_match = !regex_false.is_match(item_title);

                if is_match && not_match {
                    urls.push(item_link.to_string());
                }

                if time > mindate {
                    mindate = time;
                }
            }
        }

        dl_p(&urls, &path, verbose, parallel_downloads);

        let _wr = write_savedstate(mindate.to_rfc3339().to_string());

        if quit {
            break ();
        }

        thread::sleep(sleep);
    }
}

struct Tfile {
    name: std::path::PathBuf,
    content_type: String,
    content: std::result::Result<bytes::Bytes, reqwest::Error>,
}

#[tokio::main]
async fn dl_p(urls: &[String], path: &String, verbose: bool, parallel_downloads: usize) {
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
                let mut p = PathBuf::from(path);
                p.push(filename);
                let content = resp.bytes().await;
                Tfile {name: p, content_type: header, content: content  }
            }
        })
        .buffer_unordered(parallel_downloads);

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
                                    Err(e) => if verbose { eprintln!("Got an error: {}", e) },
                                }
                            },
                            Err(e) => if verbose { eprintln!("Got an error: {}", e) },
                        }
                    }
                    _ => if verbose { eprintln!("Rss url does not link to a torrent file.") }
                }
            }
        })
        .await;
}

fn write_savedstate(pub_date: String) -> std::io::Result<()> {
    let mut file = File::create("savedstate.dat")?;
    file.write_all(pub_date.as_bytes())?;
    Ok(())
}

fn read_savedstate() -> std::io::Result<String> {
    let mut file = File::open("savedstate.dat")?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    contents = contents.trim().to_string();
    Ok(contents)
}
