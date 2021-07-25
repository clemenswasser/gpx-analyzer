use std::{
    path::{Path, PathBuf},
    process,
};

use chrono::prelude::*;
use clap::Clap;
use rayon::prelude::*;

#[derive(Debug, Clap)]
#[clap(name = "gpx-analyzer")]
struct Opt {
    #[clap(short, long, allow_hyphen_values = true)]
    pub coordinate: String,
    #[clap(short, long)]
    pub distance: f64,
    #[clap(short = 'j', long)]
    pub threads: Option<usize>,
    #[clap(short, long)]
    pub recursive: bool,
    #[clap(name = "PATH")]
    pub path: Option<PathBuf>,
}

#[derive(Default)]
struct GpxResult {
    distance: f64,
    path: String,
    time: Option<String>,
}

fn analyze(
    path: &Path,
    lat: f64,
    lon: f64,
    deg_lat_to_dist: f64,
    deg_lon_to_dist: f64,
    distance: f64,
) -> Vec<GpxResult> {
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
                    let (found_lon, found_lat) = (
                        if let Some(attr) = e
                            .attributes()
                            .find(|e| e.as_ref().map_or(false, |attr| attr.key == b"lon"))
                            .and_then(Result::ok)
                            .and_then(|attr| attr.unescape_and_decode_value(&reader).ok())
                            .and_then(|attr| attr.parse::<f64>().ok())
                        {
                            attr
                        } else {
                            eprintln!(
                                "[WARNING] Invalid longitude in file: {}",
                                path.to_str().unwrap()
                            );
                            continue;
                        },
                        if let Some(attr) = e
                            .attributes()
                            .find(|e| e.as_ref().map_or(false, |attr| attr.key == b"lat"))
                            .and_then(Result::ok)
                            .and_then(|attr| attr.unescape_and_decode_value(&reader).ok())
                            .and_then(|attr| attr.parse::<f64>().ok())
                        {
                            attr
                        } else {
                            eprintln!(
                                "[WARNING] Invalid latitude in file: {}",
                                path.to_str().unwrap()
                            );
                            continue;
                        },
                    );

                    let d_lon = found_lon - lon;
                    let d_lat = found_lat - lat;
                    let x = (d_lon) * deg_lon_to_dist;
                    let y = (d_lat) * deg_lat_to_dist;

                    let dist = if let Some((last_x, last_y)) = last_point {
                        let d_x = x - last_x;
                        let d_y: f64 = y - last_y;

                        let a = d_y.atan2(d_x) * -1.0;

                        let dist = (-x * a.sin() + y * a.cos()).abs();

                        let last_t_x = last_x * a.cos() + last_y * a.sin();

                        let t_x = x * a.cos() + y * a.sin();

                        if (last_t_x >= 0.0 && t_x <= 0.0) || (last_t_x <= 0.0 && t_x >= 0.0) {
                            dist
                        } else {
                            f64::hypot(x, y)
                        }
                    } else {
                        f64::hypot(x, y)
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
                    last_point = Some((x, y));
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
                if new_results.is_empty() {
                    if let Some(nearest) = nearest.take() {
                        results.push(nearest);
                    }
                } else if let Some(new_closest_result) = new_results
                    .into_iter()
                    .min_by(|x, y| x.distance.partial_cmp(&y.distance).unwrap())
                {
                    results.push(new_closest_result);
                }

                return results;
            }
            Err(e) => eprintln!(
                "[Error] Unexpected XML error in file: `{}` at position: `{}` with error: `{:?}`",
                path.to_str().unwrap(),
                reader.buffer_position(),
                e
            ),
            _ => (),
        }
    }
}

fn read_dir_db(path: impl AsRef<Path>, analyze_db: &mut Vec<PathBuf>, recursive: bool) {
    if let Ok(dir_entrys) = std::fs::read_dir(path) {
        for dir_entry in dir_entrys {
            let dir_entry = dir_entry.unwrap();
            if recursive && dir_entry.metadata().unwrap().is_dir() {
                read_dir_db(dir_entry.path(), analyze_db, recursive);
            } else if let Some(ext) = dir_entry.path().extension() {
                if ext.eq("gpx") {
                    analyze_db.push(dir_entry.path());
                }
            }
        }
    }
}

fn parse_deg_min_sec(mut input: String) -> f64 {
    input = input.trim().to_string();
    let first = input.remove(0);
    let south_west = first.eq(&'S') || first.eq(&'W');
    input = input.trim().to_string();

    let split = input.split(' ').map(str::to_string).collect::<Vec<_>>();

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
    if let Some(time) = result
        .time
        .as_ref()
        .and_then(|time_str| time_str.parse::<DateTime<Utc>>().ok())
    {
        let time_in_local_timezone = time.with_timezone(&chrono::offset::Local);
        println!(
            "{:.1};{};{};{}",
            result.distance,
            time_in_local_timezone.time().to_string(),
            time_in_local_timezone.date().to_string(),
            result.path
        );
    } else {
        println!("{:.1};;;{}", result.distance, result.path);
    }
}

fn main() {
    let opt = Opt::parse();

    let (latitude, longitude) = {
        if let Some((Ok(latitude), Ok(longitude))) =
            opt.coordinate
                .split_once(' ')
                .map(|(latitude_str, longitude_str)| {
                    (
                        latitude_str.replace(",", "").parse::<f64>(),
                        longitude_str.replace(",", "").parse::<f64>(),
                    )
                })
        {
            (latitude, longitude)
        } else {
            let (first, second) = opt.coordinate.split_at(opt.coordinate.len() / 2);

            (
                parse_deg_min_sec(first.to_string()),
                parse_deg_min_sec(second.to_string()),
            )
        }
    };

    // WGS-84: https://en.wikipedia.org/wiki/World_Geodetic_System#WGS84

    let deg_lat_to_dist: f64 = 6_378_137.0_f64.to_radians() * longitude.to_radians().cos();
    let deg_lon_to_dist: f64 = 6_356_752.314_245_18_f64.to_radians() * latitude.to_radians().cos();

    println!("{}, {}", latitude, longitude);
    println!(
        "{} {}° {} {} {}° {}",
        if latitude.is_sign_negative() {
            "S"
        } else {
            "N"
        },
        latitude as u64,
        latitude % 1.0 * 60.0,
        if longitude.is_sign_negative() {
            "W"
        } else {
            "E"
        },
        longitude.abs() as u64,
        longitude.abs() % 1.0 * 60.0,
    );

    if let Some(threads) = opt.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build_global()
            .unwrap();
    }

    let path = opt
        .path
        .unwrap_or_else(|| std::env::current_dir().expect("Get current dir"));

    if std::fs::metadata(&path).is_err() {
        eprintln!(
            "[Error] The Path `{}` does not exist",
            path.to_str().unwrap()
        );
        process::exit(-1);
    }

    let analyze_db = {
        let mut analyze_db = Vec::new();
        read_dir_db(&path, &mut analyze_db, opt.recursive);
        analyze_db
    };

    println!("Found {} gpx file(s)", analyze_db.len());
    println!("Searching in `{}`...", path.to_str().unwrap(),);

    let distance = opt.distance;
    let mut results = analyze_db
        .par_iter()
        .flat_map(|gpx_file| {
            analyze(
                gpx_file,
                latitude,
                longitude,
                deg_lat_to_dist,
                deg_lon_to_dist,
                distance,
            )
        })
        .collect::<Vec<_>>();

    results
        .sort_by(|result_1, result_2| result_1.distance.partial_cmp(&result_2.distance).unwrap());

    let distance = opt.distance;

    let results_within_distance = results
        .par_iter()
        .filter(|result| result.distance <= distance)
        .collect::<Vec<_>>();

    if !results_within_distance.is_empty() {
        println!(
            "Found {} point(s) within distance ({}m):\n\
            dist;time;date;path",
            results_within_distance.len(),
            opt.distance
        );

        let out_range_index = results_within_distance.len();

        results_within_distance.into_iter().for_each(print_result);

        if let Some(result) = results.get(out_range_index) {
            println!("Nearest point out of distance was:");
            print_result(result);
        }
    } else if let Some(first_result) = results.first() {
        println!(
            "Did not find any point within distance.\n\
            Closest was:\n\
            dist;time;date;path"
        );
        print_result(first_result);
    } else {
        println!("Did not find any points.");
    }
}
