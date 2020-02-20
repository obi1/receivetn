use std::{process, thread};
use receivetn::Conf;

fn main() {
    let config = Conf::new("config.toml").unwrap_or_else(|error| {
        eprintln!("Error: {}.", error);
        process::exit(1);
    });
    
    loop {
        let date = receivetn::read_savedstate();
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

        if let Err(error) = receivetn::write_savedstate(urls.mindate) {
            eprintln!("Warning: {}. Can't write to file savedstate.dat.", error);
        }

        if config.quit {
            break;
        }

        thread::sleep(config.sleep);
    }
}
