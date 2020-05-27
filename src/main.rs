use geo::prelude::*;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "gpx-analyzer")]
struct Opt {
    #[structopt(long, conflicts_with = "coordinates")]
    pub longitude: Option<String>,
    #[structopt(long, conflicts_with = "coordinates")]
    pub latitude: Option<String>,
    #[structopt(short, long, conflicts_with = "longitude latitude")]
    pub coordinates: Option<String>,
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
    time: Option<String>,
}

fn analyze(path: &std::path::PathBuf, lon: f64, lat: f64, distance: f64) -> Vec<Result> {
    let mut reader = quick_xml::Reader::from_file(&path).unwrap();
    reader.trim_text(true);
    let mut buf = Vec::new();

    let mut results = Vec::new();
    let mut nearest_dist = std::f64::MAX;
    let mut time_update = false;
    let mut nearest_time = String::new();

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
                        time_update = true;
                    }
                    if dist <= distance {
                        results.push(Result {
                            distance: dist,
                            path: path.to_str().unwrap().to_string(),
                            time: None,
                        });
                    }
                } else if e.name().eq(b"time") && time_update {
                    let time = reader.read_text(e.name(), &mut Vec::new()).unwrap();
                    nearest_time = time.clone();
                    if let Some(last) = results.last_mut() {
                        if last.time.is_none() {
                            last.time = Some(time);
                        }
                    }
                }
            }

            Ok(quick_xml::events::Event::Text(_)) => {}
            Ok(quick_xml::events::Event::Eof) => {
                if results.len() > 0 {
                    return results;
                } else {
                    return vec![Result {
                        distance: nearest_dist,
                        path: path.to_str().unwrap().into(),
                        time: Some(nearest_time),
                    }];
                }
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
        panic!("Your specified Path is neither a directory nor a gpx file");
    }
}

fn parse_deg_min_sec(input: String) -> f64 {
    let mut split = input
        .split(" ")
        .map(|str| str.to_string())
        .collect::<Vec<_>>();
    let mut param = split.remove(0);
    let dir_param = param.remove(0);
    let south = dir_param.eq(&'S') || dir_param.eq(&'W');

    let mut out =
        param.parse::<f64>().unwrap() + split.get(0).unwrap().parse::<f64>().unwrap() / 60.0;
    if south {
        out *= -1.0;
    };
    out
}

fn print_result(result: &Result) {
    if result.time.is_some() {
        let time = result.time.as_ref().unwrap();
        let mut time_split = time.split("T");
        let date = time_split.next().unwrap();
        let mut time = time_split.next().unwrap().to_string();
        time.remove(time.len() - 1);
        println!("{:.1};{};{};{}", result.distance, time, date, result.path);
    } else {
        println!("{:.1};{};{};{}", result.distance, "00:00:00", "0000-00-00", result.path);
    }
}

fn main() {
    let opt = Opt::from_args();

    let (latitude, longitude) =
        if let (Some(longitude), Some(latitude)) = (opt.longitude, opt.latitude) {
            (
                match latitude.parse::<f64>() {
                    Ok(latitude) => latitude,
                    Err(_) => parse_deg_min_sec(latitude),
                },
                match longitude.parse::<f64>() {
                    Ok(longitude) => longitude,
                    Err(_) => parse_deg_min_sec(longitude),
                },
            )
        } else if let Some(coordinates) = opt.coordinates {
            let split = coordinates
                .split(" ")
                .map(|split| split.to_string())
                .collect::<Vec<_>>();
            if let (Ok(latitude), Ok(longitude)) = (
                split.get(0).unwrap().parse::<f64>(),
                split.get(1).unwrap().parse::<f64>(),
            ) {
                (latitude, longitude)
            } else {
                let first = split.get(0).unwrap().to_string() + " " + split.get(1).unwrap();
                let second = split.get(2).unwrap().to_string() + " " + split.get(3).unwrap();
                (parse_deg_min_sec(first), parse_deg_min_sec(second))
            }
        } else {
            panic!("You must specify either --longitude and --latitude or --coordinates");
        };

    dbg!(latitude, longitude);

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

    let distance = opt.distance;
    let analysis_results = analyze_db
        .par_iter()
        .map(|gpx_file| analyze(gpx_file, longitude, latitude, distance))
        .collect::<Vec<_>>();
    let mut results = Vec::with_capacity(analysis_results.iter().map(|result_vec| result_vec.len()).sum());
    analysis_results.iter().for_each(|result| results.extend(result));

    results
        .sort_by(|result_1, result_2| result_1.distance.partial_cmp(&result_2.distance).unwrap());

    let distance = opt.distance;
    let filtered_results = results
        .par_iter()
        .filter(|result| result.distance <= distance)
        .collect::<Vec<_>>();

    if filtered_results.len() > 0 {
        println!(
            "Found {} point(s) in your defined minimum distance ({}m):\n\
            dist;time;date;path",
            filtered_results.len(),
            opt.distance
        );
        filtered_results
            .iter()
            .for_each(|result| print_result(result));

        let out_range_index = filtered_results.len();
        std::mem::drop(filtered_results);

        if let Some(result) = results.get(out_range_index) {
            println!("Nearest point out of distance was:");
            print_result(result);
        }
    } else {
        println!(
            "Did not find any point in your defined minimum distance.\n\
            Closest was:\n\
            dist;time;date;path"
        );
        std::mem::drop(filtered_results);
        print_result(results.first().unwrap());
    }
}
