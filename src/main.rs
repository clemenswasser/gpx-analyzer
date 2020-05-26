use geo::algorithm::haversine_distance::HaversineDistance;
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
    #[structopt(short, long)]
    pub path: Option<std::path::PathBuf>,
    #[structopt(short = "j", long)]
    pub threads: Option<usize>,
}

struct Result {
    distance: f64,
    path: String,
}

fn analyze(path: &std::path::PathBuf, lon: f64, lat: f64) -> Result {
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

            Ok(quick_xml::events::Event::Text(_)) => {}
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

use rayon::prelude::*;

fn read_dir_db(path: std::path::PathBuf) -> Vec<std::path::PathBuf> {
    if std::fs::metadata(&path).unwrap().is_dir() {
        let dir_entrys = std::fs::read_dir(path).unwrap();
        let mut results = Vec::<std::path::PathBuf>::new();
        for dir_entry in dir_entrys {
            let dir_entry = dir_entry.unwrap();
            if dir_entry.metadata().unwrap().is_dir() {
                results.extend(read_dir_db(dir_entry.path()));
            } else if let Some(ext) = dir_entry.path().extension() {
                if ext.eq("gpx") {
                    results.push(dir_entry.path());
                }
            }
        }
        results
    } else if path.extension().unwrap().eq("gpx") {
        vec![path]
    } else {
        panic!();
    }
}

fn main() {
    let opt = Opt::from_args();

    if opt.threads.is_some() {
        rayon::ThreadPoolBuilder::new()
            .num_threads(opt.threads.unwrap())
            .build_global()
            .unwrap();
    }

    let analyze_db = read_dir_db(opt.path.unwrap_or(".".into()));

    println!(
        "Found {} gpx file(s).\n\
        Searching...",
        analyze_db.len()
    );

    let lon = opt.lon;
    let lat = opt.lat;
    let mut results = analyze_db
        .par_iter()
        .map(|gpx_file| analyze(gpx_file, lon, lat))
        .collect::<Vec<_>>();

    results
        .sort_by(|result_1, result_2| result_1.distance.partial_cmp(&result_2.distance).unwrap());

    let distance = opt.distance;
    let filtered_results = results
        .par_iter()
        .filter(|result| result.distance <= distance)
        .collect::<Vec<_>>();

    if filtered_results.len() > 0 {
        println!(
            "Found {} point(s) in your defined minimum distance ({}m):",
            filtered_results.len(),
            opt.distance
        );
        filtered_results
            .iter()
            .for_each(|result| println!("{:.1}m in file: {}", result.distance, result.path));

        let out_range_index = filtered_results.len();
        std::mem::drop(filtered_results);

        if let Some(result) = results.get(out_range_index) {
            println!(
                "Nearest point out of distance was:\n\
                {:.1}m in file: {}",
                result.distance, result.path
            );
        }
    } else {
        println!(
            "Did not find any point in your defined minimum distance.\n\
            Closest was:"
        );
        std::mem::drop(filtered_results);
        let result = results.first().unwrap();
        println!("{:.1}m in file: {}", result.distance, result.path);
    }
}
