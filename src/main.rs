use chrono::prelude::*;
use geo::prelude::*;
use rayon::prelude::*;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "gpx-analyzer")]
struct Opt {
    #[structopt(long, conflicts_with = "coordinates")]
    pub longitude: Option<String>,
    #[structopt(long, conflicts_with = "coordinates")]
    pub latitude: Option<String>,
    #[structopt(short, long, conflicts_with = "longitude latitude")]
    pub coordinate: Option<String>,
    #[structopt(short, long)]
    pub distance: f64,
    #[structopt(short = "j", long)]
    pub threads: Option<usize>,
    #[structopt(name = "PATH")]
    pub path: Option<std::path::PathBuf>,
}

struct GpxResult {
    distance: f64,
    path: String,
    time: Option<String>,
}

fn analyze(path: &std::path::PathBuf, lon: f64, lat: f64, distance: f64) -> Vec<GpxResult> {
    let mut reader = quick_xml::Reader::from_file(&path).unwrap();
    reader.trim_text(true);
    let mut buf = Vec::new();

    let mut results: Vec<GpxResult> = Vec::new();
    let mut new_results: Vec<GpxResult> = Vec::new();
    let mut nearest: Option<GpxResult> = None;
    let mut time_update = false;
    let mut in_time = false;
    let mut searching_time_for = std::usize::MAX;
    let mut last_point = None;

    loop {
        match reader.read_event(&mut buf) {
            Ok(quick_xml::events::Event::Start(ref e)) => {
                if e.name().eq(b"trkpt") {
                    let pt = geo_types::Point::new(
                        match e.attributes().find(|e| match e {
                            Ok(attr) => attr.key == b"lon",
                            Err(_) => false,
                        }) {
                            Some(attr) => {
                                match match match attr {
                                    Ok(val) => val,
                                    Err(_) => {
                                        eprintln!(
                                            "[WARNING] Invalid longitude in file: {}",
                                            path.to_str().unwrap()
                                        );
                                        continue;
                                    }
                                }
                                .unescape_and_decode_value(&reader)
                                {
                                    Ok(val) => val,
                                    Err(_) => {
                                        eprintln!(
                                            "[WARNING] Invalid longitude in file: {}",
                                            path.to_str().unwrap()
                                        );
                                        continue;
                                    }
                                }
                                .parse::<f64>()
                                {
                                    Ok(val) => val,
                                    Err(_) => {
                                        eprintln!(
                                            "[WARNING] Invalid longitude in file: {}",
                                            path.to_str().unwrap()
                                        );
                                        continue;
                                    }
                                }
                            }
                            None => continue,
                        },
                        match e.attributes().find(|e| match e {
                            Ok(attr) => attr.key == b"lat",
                            Err(_) => false,
                        }) {
                            Some(attr) => {
                                match match match attr {
                                    Ok(val) => val,
                                    Err(_) => {
                                        eprintln!(
                                            "[WARNING] Invalid latitude in file: {}",
                                            path.to_str().unwrap()
                                        );
                                        continue;
                                    }
                                }
                                .unescape_and_decode_value(&reader)
                                {
                                    Ok(val) => val,
                                    Err(_) => {
                                        eprintln!(
                                            "[WARNING] Invalid latitude in file: {}",
                                            path.to_str().unwrap()
                                        );
                                        continue;
                                    }
                                }
                                .parse::<f64>()
                                {
                                    Ok(val) => val,
                                    Err(_) => {
                                        eprintln!(
                                            "[WARNING] Invalid latitude in file: {}",
                                            path.to_str().unwrap()
                                        );
                                        continue;
                                    }
                                }
                            }
                            None => continue,
                        },
                    );

                    let dist = match last_point {
                        Some(last_point) => {
                            let line = geo_types::Line::new(last_point, pt);
                            geo_types::Point::new(lon, lat).euclidean_distance(&line)
                        }
                        None => geo_types::Point::new(lon, lat).haversine_distance(&pt),
                    };

                    if (nearest.is_none() || nearest.as_ref().unwrap().distance > dist)
                        && dist > distance
                    {
                        nearest = Some(GpxResult {
                            distance: dist,
                            path: path.to_str().unwrap().to_string(),
                            time: None,
                        });
                        time_update = true;
                    }

                    if dist > distance && !new_results.is_empty() {
                        new_results.sort_by(|result_1: &GpxResult, result_2: &GpxResult| {
                            result_1.distance.partial_cmp(&result_2.distance).unwrap()
                        });
                        results.push(new_results.remove(0));
                        new_results.clear();
                    }

                    if dist <= distance {
                        new_results.push(GpxResult {
                            distance: dist,
                            path: path.to_str().unwrap().to_string(),
                            time: None,
                        });
                        searching_time_for = new_results.len() - 1;
                    }
                    last_point = Some(pt);
                } else if e.name().eq(b"time") {
                    in_time = true;
                }
            }
            Ok(quick_xml::events::Event::End(e)) => {
                if e.name().eq(b"trkpt") {
                    searching_time_for = std::usize::MAX;
                }
                if e.name().eq(b"time") {
                    in_time = false;
                }
            }
            Ok(quick_xml::events::Event::Text(e)) => {
                if in_time {
                    let time = e.unescape_and_decode(&reader).unwrap();
                    if !time.eq("") {
                        if let Some(time_for) = new_results.get_mut(searching_time_for) {
                            time_for.time = Some(time);
                        } else if time_update {
                            nearest.as_mut().unwrap().time = Some(time);
                        }
                    }
                    time_update = false;
                }
            }
            Ok(quick_xml::events::Event::Eof) => {
                let capacity = if nearest.is_some() {
                    results.len() + std::cmp::min(new_results.len(), 1) + 1
                } else {
                    results.len() + std::cmp::min(new_results.len(), 1)
                };
                if capacity == 0 {
                    return vec![];
                }
                let mut out = Vec::<GpxResult>::with_capacity(capacity);

                if results.is_empty() && new_results.is_empty() {
                    if let Some(nearest) = nearest.take() {
                        out.push(nearest);
                    }
                }

                if !results.is_empty() {
                    out.extend(results);
                }
                if !new_results.is_empty() {
                    new_results.sort_by(|result_1: &GpxResult, result_2: &GpxResult| {
                        result_1.distance.partial_cmp(&result_2.distance).unwrap()
                    });
                    out.push(new_results.remove(0));
                }

                return out;
            }
            Err(e) => eprintln!(
                "[Error] file: \"{}\"; position: {}; {:?}",
                path.to_str().unwrap(),
                reader.buffer_position(),
                e
            ),
            _ => (),
        }
    }
}

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

fn parse_deg_min_sec(mut input: String) -> f64 {
    input = input.trim().to_string();
    let first = input.remove(0);
    let south_west = first.eq(&'S') || first.eq(&'W');
    input = input.trim().to_string();

    let split = input
        .split(' ')
        .map(|str| str.to_string())
        .collect::<Vec<_>>();

    let mut out = split
        .get(0)
        .unwrap()
        .replace("°", "")
        .parse::<f64>()
        .unwrap()
        + split.get(1).unwrap().parse::<f64>().unwrap() / 60.0;
    if south_west {
        out *= -1.0;
    };
    out
}

fn print_result(result: &GpxResult) {
    if result.time.is_some() {
        let time = result
            .time
            .as_ref()
            .unwrap()
            .parse::<DateTime<Utc>>()
            .unwrap()
            .with_timezone(&chrono::offset::Local);
        println!(
            "{:.1};{};{};{}",
            result.distance,
            time.time().to_string(),
            time.date().to_string(),
            result.path
        );
    } else {
        println!(
            "{:.1};{};{};{}",
            result.distance, "00:00:00", "0000-00-00", result.path
        );
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
        } else if let Some(coordinates) = opt.coordinate {
            let split = coordinates
                .split(' ')
                .map(|split| split.to_string())
                .collect::<Vec<_>>();
            if let (Ok(latitude), Ok(longitude)) = (
                split.get(0).unwrap().replace(",", "").parse::<f64>(),
                split.get(1).unwrap().replace(",", "").parse::<f64>(),
            ) {
                (latitude, longitude)
            } else {
                let (first, second) = coordinates.split_at(coordinates.len() / 2);

                (
                    parse_deg_min_sec(first.to_string()),
                    parse_deg_min_sec(second.to_string()),
                )
            }
        } else {
            panic!("You must specify either --longitude and --latitude or --coordinates");
        };

    println!("{}, {}", latitude, longitude);
    println!(
        "{} {}° {:.6} {} {}° {:.6}",
        if latitude.is_sign_negative() {
            "S"
        } else {
            "N"
        },
        latitude as u64,
        latitude % 1.0 * 60.0,
        if latitude.is_sign_negative() {
            "W"
        } else {
            "E"
        },
        longitude as u64,
        longitude % 1.0 * 60.0,
    );

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
    let mut results = Vec::with_capacity(
        analysis_results
            .iter()
            .map(|result_vec| result_vec.len())
            .sum(),
    );
    analysis_results.iter().for_each(|result| {
        if !result.is_empty() {
            results.extend(result);
        }
    });

    results
        .sort_by(|result_1, result_2| result_1.distance.partial_cmp(&result_2.distance).unwrap());

    let distance = opt.distance;
    let filtered_results = results
        .par_iter()
        .filter(|result| result.distance <= distance)
        .collect::<Vec<_>>();

    if !filtered_results.is_empty() {
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
    } else if !results.is_empty() {
        println!(
            "Did not find any point in your defined minimum distance.\n\
            Closest was:\n\
            dist;time;date;path"
        );
        std::mem::drop(filtered_results);
        print_result(results.first().unwrap());
    } else {
        println!(
            "Did not find any points."
        );
    }
}
