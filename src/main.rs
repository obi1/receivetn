// receivetn
// Copyright (C) 2020  Tadej Obrstar
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
// 
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use std::path::PathBuf;
use receivetn::Conf;
use std::{process, thread};
use structopt::StructOpt;

#[derive(StructOpt)]
struct Opt {
    #[structopt(parse(from_os_str), short = "c", long = "config", default_value = "config.toml")]
    config_file: PathBuf,
}

fn main() {
    let opt = Opt::from_args();

    let configs = Conf::from_file(&opt.config_file.to_str().unwrap()).unwrap_or_else(|error| {
        eprintln!("Error: {}.", error);
        process::exit(1);
    });

    let mut handles: Vec<thread::JoinHandle<_>> = vec![];

    for config in configs {
        let handle = thread::spawn(move || {
            loop {
                let date = receivetn::read_savedstate(&config.name, &config.verbose);
                let urls = match receivetn::get_new_urls(&config, &date) {
                    Ok(c) => c,
                    Err(error) => {
                        if config.verbose {
                            println!("Error getting channel data: {}.", error);
                        }
                        if config.quit {
                            break
                        }
                        else {
                            continue
                        }
                    }
                }; 
                
                receivetn::download_files(&urls.urls, &config);
        
                if let Err(error) = receivetn::write_savedstate(&config.name, &urls.mindate) {
                    if config.verbose {
                        eprintln!("Warning: {}. Can't write to file state_{}.dat.", error, &config.name);
                    }
                }
        
                if config.quit {
                    break;
                }
        
                thread::sleep(config.sleep);
            }
        });
        handles.push(handle);
    }

    for h in handles {
        h.join().unwrap();
    }
}
