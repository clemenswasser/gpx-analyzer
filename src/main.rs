use geo::algorithm::haversine_distance::HaversineDistance;
use std::io::Write;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "gpx-analyzer")]
struct Opt {
    #[structopt(long)]
    pub lon: f64,
    #[structopt(long)]
    pub lat: f64,
    #[structopt(short, long)]
    pub distance: f64,
    #[structopt(short = "j", long, default_value = "1")]
    pub threads: usize,
}

struct Result {
    distance: f64,
    path: String,
    //time: std::
}

fn analyze(path: std::path::PathBuf, lon: f64, lat: f64) -> Result {
    let mut reader = quick_xml::Reader::from_file(&path).unwrap();
    reader.trim_text(true);
    let mut buf = Vec::new();

    let mut nearest_dist = std::f64::MAX;

    loop {
        match reader.read_event(&mut buf) {
            Ok(quick_xml::events::Event::Start(ref e)) => {
                if e.name().eq(b"trkpt") {
                    let pt = geo_types::Point::new(
                        match e.attributes().find(|e| match e {
                            Ok(attr) => attr.key == b"lon",
                            Err(_) => false,
                        }) {
                            Some(attr) => attr
                                .unwrap()
                                .unescape_and_decode_value(&reader)
                                .unwrap()
                                .parse::<f64>()
                                .unwrap(),
                            None => continue,
                        },
                        match e.attributes().find(|e| match e {
                            Ok(attr) => attr.key == b"lat",
                            Err(_) => false,
                        }) {
                            Some(attr) => attr
                                .unwrap()
                                .unescape_and_decode_value(&reader)
                                .unwrap()
                                .parse::<f64>()
                                .unwrap(),
                            None => continue,
                        },
                    );

                    let dist = geo_types::Point::new(lon, lat).haversine_distance(&pt);

                    if nearest_dist > dist {
                        nearest_dist = dist;
                    }
                }
            }

            Ok(quick_xml::events::Event::Text(e)) => {
                //println!("{}", e.unescape_and_decode(&reader).unwrap());
            }
            Ok(quick_xml::events::Event::Eof) => {
                return Result {
                    distance: nearest_dist,
                    path: path.to_str().unwrap().into(),
                };
            }
            Err(e) => panic!(
                "Error in file \"{}\", at position {}: {:?}",
                path.to_str().unwrap(),
                reader.buffer_position(),
                e
            ),
            _ => (),
        }
    }
}

fn read_dir(
    entries: std::fs::ReadDir,
    lon: f64,
    lat: f64,
    distance: f64,
    all_tasks: std::sync::Arc<std::sync::Mutex<u64>>,
    finished_tasks: std::sync::Arc<std::sync::Mutex<u64>>,
    nearest_out_of_dist: std::sync::Arc<std::sync::Mutex<Result>>,
    search_result_send: std::sync::Arc<std::sync::Mutex<std::sync::mpsc::Sender<Result>>>,
) {
    entries.for_each(move |entry| {
        let all_tasks = all_tasks.clone();
        let finished_tasks = finished_tasks.clone();
        let nearest_out_of_dist = nearest_out_of_dist.clone();
        let search_result_send = search_result_send.clone();
        rayon::scope(move |s| {
            let entry = entry.unwrap();
            let metadata = entry.metadata().unwrap();
            if metadata.is_dir() {
                s.spawn(move |_| {
                    read_dir(
                        std::fs::read_dir((&entry).path()).unwrap(),
                        lon,
                        lat,
                        distance,
                        all_tasks,
                        finished_tasks,
                        nearest_out_of_dist,
                        search_result_send,
                    )
                });
            } else if entry.path().to_str().unwrap().ends_with(".gpx") {
                *all_tasks.lock().unwrap() += 1;
                s.spawn(move |_| {
                    let res = analyze(entry.path(), lon, lat);
                    if res.distance <= distance {
                        search_result_send.lock().unwrap().send(res).unwrap();
                    } else {
                        let mut dist = nearest_out_of_dist.lock().unwrap();
                        if dist.distance > res.distance {
                            dist.distance = res.distance;
                            dist.path = res.path;
                        }
                    }

                    *finished_tasks.lock().unwrap() += 1;
                });
            }
        })
    });
}

fn main() {
    let opt = Opt::from_args();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(opt.threads + 1)
        .build()
        .unwrap();
    let all_tasks = std::sync::Arc::new(std::sync::Mutex::new(0));
    let finished_tasks = std::sync::Arc::new(std::sync::Mutex::new(0));
    let (send, recv) = std::sync::mpsc::channel::<()>();
    let send = std::sync::Arc::new(std::sync::Mutex::new(send));
    let recv = std::sync::Arc::new(std::sync::Mutex::new(recv));

    let nearest_out_of_dist = std::sync::Arc::new(std::sync::Mutex::new(Result {
        distance: std::f64::MAX,
        path: String::new(),
    }));

    let (search_result_send, search_results_recv) = std::sync::mpsc::channel::<Result>();
    let search_result_send = std::sync::Arc::new(std::sync::Mutex::new(search_result_send));
    let search_results_recv = std::sync::Arc::new(std::sync::Mutex::new(search_results_recv));
    pool.scope(|s| {
        s.spawn(|_| {
            let recv = recv.clone();
            while match recv
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_millis(250))
            {
                Ok(_) => false,
                Err(_) => true,
            } {
                print!(
                    "{}/{}\r",
                    finished_tasks.lock().unwrap(),
                    all_tasks.lock().unwrap()
                );
                std::io::stdout().flush().unwrap();
            }

            let results = search_results_recv
                .lock()
                .unwrap()
                .try_iter()
                .collect::<Vec<_>>();

            if results.len() > 0 {
                println!(
                    "Found {} Points in your defined minimum distance ({}m):",
                    results.len(),
                    opt.distance
                );
                results.iter().for_each(|result| {
                    println!("{:.1}m in file: {}", result.distance, result.path)
                });
            } else {
                println!(
                    "Did not find any Points in your defined minimum distance.\n{}",
                    "Closest was:"
                );
                let dist = nearest_out_of_dist.lock().unwrap();
                println!("{:.1}m in file: {}", dist.distance, dist.path,);
            }
        });
        s.spawn(|_| {
            let send = send.clone();
            read_dir(
                std::fs::read_dir(".").unwrap(),
                opt.lon,
                opt.lat,
                opt.distance,
                all_tasks.clone(),
                finished_tasks.clone(),
                nearest_out_of_dist.clone(),
                search_result_send.clone(),
            );
            send.lock().unwrap().send(()).unwrap();
        });
    });
}
