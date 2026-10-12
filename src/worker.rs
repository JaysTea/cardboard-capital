//! Background threads: one sequential, rate-limited thread for the Scryfall API and
//! a small pool for image downloads. Results come back to the UI over a channel.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::fx::{self, FxQuote};
use crate::import::{self, ImportOptions, ImportResult, ImportSource};
use crate::model::UpdateChannel;
use crate::update::{self, UpdateInfo};
use crate::scryfall::{Card, Client};

pub enum Job {
    CheckUpdate {
        channel: UpdateChannel,
        manual: bool,
    },
    InstallUpdate {
        info: UpdateInfo,
        data_dir: PathBuf,
    },
    Autocomplete {
        seq: u64,
        query: String,
    },
    SearchName {
        seq: u64,
        name: String,
    },
    SearchSetNumber {
        seq: u64,
        set: String,
        numbers: Vec<String>,
    },
    RefreshPrices {
        ids: Vec<String>,
    },
    FetchFx {
        currency: String,
    },
    Import {
        seq: u64,
        source: ImportSource,
        opts: ImportOptions,
    },
}

pub struct SearchResult {
    pub prints: Vec<Card>,
    /// Set when the lookup was for an exact printing (set + number).
    pub exact_id: Option<String>,
}

pub enum Reply {
    Update {
        manual: bool,
        result: Result<Option<UpdateInfo>, String>,
    },
    Installed(Result<PathBuf, String>),
    Autocomplete {
        seq: u64,
        result: Result<Vec<String>, String>,
    },
    Search {
        seq: u64,
        result: Result<SearchResult, String>,
    },
    Prices(Result<Vec<Card>, String>),
    Fx(Result<FxQuote, String>),
    Import {
        seq: u64,
        result: Result<ImportResult, String>,
    },
    Image {
        url: String,
        result: Result<egui::ColorImage, String>,
    },
}

pub struct Worker {
    pub jobs: Sender<Job>,
    pub images: Sender<String>,
    pub replies: Receiver<Reply>,
}

const IMAGE_THREADS: usize = 4;

pub fn spawn(ctx: egui::Context, cache_dir: PathBuf) -> Worker {
    let client = Arc::new(Client::new());
    let (job_tx, job_rx) = channel::<Job>();
    let (img_tx, img_rx) = channel::<String>();
    let (reply_tx, reply_rx) = channel::<Reply>();

    {
        let client = client.clone();
        let reply_tx = reply_tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            while let Ok(job) = job_rx.recv() {
                let reply = run_job(&client, job);
                if reply_tx.send(reply).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
    }

    let _ = std::fs::create_dir_all(&cache_dir);
    let img_rx = Arc::new(Mutex::new(img_rx));
    for _ in 0..IMAGE_THREADS {
        let client = client.clone();
        let reply_tx = reply_tx.clone();
        let ctx = ctx.clone();
        let img_rx = img_rx.clone();
        let cache_dir = cache_dir.clone();
        thread::spawn(move || loop {
            let url = {
                let rx = img_rx.lock().unwrap();
                match rx.recv() {
                    Ok(u) => u,
                    Err(_) => break,
                }
            };
            let result = load_image(&client, &cache_dir, &url);
            if reply_tx.send(Reply::Image { url, result }).is_err() {
                break;
            }
            ctx.request_repaint();
        });
    }

    Worker {
        jobs: job_tx,
        images: img_tx,
        replies: reply_rx,
    }
}

fn run_job(client: &Client, job: Job) -> Reply {
    match job {
        Job::Autocomplete { seq, query } => Reply::Autocomplete {
            seq,
            result: client.autocomplete(&query),
        },
        Job::SearchName { seq, name } => Reply::Search {
            seq,
            result: client.search_by_name(&name).map(|prints| SearchResult {
                prints,
                exact_id: None,
            }),
        },
        Job::SearchSetNumber { seq, set, numbers } => {
            // Try each spelling of the number (e.g. "105", then "0105") until one exists.
            let mut found = Err(format!(
                "No card {} #{} found.",
                set.to_uppercase(),
                numbers.first().map_or("", |n| n.as_str())
            ));
            for number in &numbers {
                match client.card_by_set_number(&set, number) {
                    Ok(card) => {
                        found = Ok(card);
                        break;
                    }
                    Err(e) if number == &numbers[0] => found = Err(e),
                    Err(_) => {}
                }
            }
            let result = found.map(|card| {
                let mut prints = client.prints_of(&card).unwrap_or_default();
                if !prints.iter().any(|c| c.id == card.id) {
                    prints.insert(0, card.clone());
                }
                SearchResult {
                    prints,
                    exact_id: Some(card.id),
                }
            });
            Reply::Search { seq, result }
        }
        Job::RefreshPrices { ids } => Reply::Prices(client.cards_by_id(&ids)),
        Job::FetchFx { currency } => Reply::Fx(fx::fetch_rate(client.agent(), &currency)),
        Job::Import { seq, source, opts } => Reply::Import {
            seq,
            result: import::run(client, source, opts),
        },
        Job::CheckUpdate { channel, manual } => Reply::Update {
            manual,
            result: update::check(client.agent(), channel),
        },
        Job::InstallUpdate { info, data_dir } => {
            Reply::Installed(update::install(client.agent(), &info, &data_dir))
        }
    }
}

fn cache_path(cache_dir: &Path, url: &str) -> PathBuf {
    let mut h = DefaultHasher::new();
    url.hash(&mut h);
    cache_dir.join(format!("{:016x}.img", h.finish()))
}

fn decode(bytes: &[u8]) -> Result<egui::ColorImage, String> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| e.to_string())?
        .to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Ok(egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}

fn load_image(client: &Client, cache_dir: &Path, url: &str) -> Result<egui::ColorImage, String> {
    let path = cache_path(cache_dir, url);
    if let Ok(bytes) = std::fs::read(&path) {
        match decode(&bytes) {
            Ok(img) => return Ok(img),
            Err(_) => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    let bytes = client.fetch_bytes(url)?;
    let img = decode(&bytes)?;
    let _ = std::fs::write(&path, &bytes);
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scryfall::{parse_set_code, printed_number_candidates};

    /// `cargo test -- --ignored live`
    #[test]
    #[ignore]
    fn live_printed_number_lookup() {
        let client = Client::new();
        for (set, printed, expected) in [
            ("SOS • EN", "M 0105", "Withering Curse"),
            ("sos", "0105", "Withering Curse"),
            ("dmu", "107/281", "Sheoldred, the Apocalypse"),
            ("dmu", "107", "Sheoldred, the Apocalypse"),
        ] {
            let job = Job::SearchSetNumber {
                seq: 1,
                set: parse_set_code(set),
                numbers: printed_number_candidates(printed),
            };
            let Reply::Search { result, .. } = run_job(&client, job) else {
                panic!("wrong reply")
            };
            let r = result.unwrap_or_else(|e| panic!("{set} {printed}: {e}"));
            let card = r
                .prints
                .iter()
                .find(|c| Some(&c.id) == r.exact_id.as_ref())
                .unwrap();
            assert_eq!(card.name, expected, "{set} {printed}");
        }
    }
}
