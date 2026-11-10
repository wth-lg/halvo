use oar_ocr::core::config::OrtSessionConfig;
use oar_ocr::domain::tasks::TextDetectionConfig;
use oar_ocr::oarocr::{OAROCRBuilder, OAROCR};
use r2_probes::frame::Frame;
use r2_probes::suite::{extraction, print_answer};
use r2_probes::sys;
use r2_probes::trial::{How, Record};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Instant;

const THREADS: usize = 1;
const SCORE_THRESHOLD: f32 = 0.2;
const BOX_THRESHOLD: f32 = 0.4;
const UNCLIP_RATIO: f32 = 1.4;

fn load(dylib: &Path, det: &Path, rec: &Path, dict: &Path) -> Result<OAROCR, String> {
    let committed = ort::init_from(dylib)
        .map_err(|error| error.to_string())?
        .with_name("r2-probes")
        .with_telemetry(false)
        .commit();
    if !committed {
        return Err("an ONNX Runtime environment already existed".to_string());
    }
    let session = OrtSessionConfig::new()
        .with_intra_threads(THREADS)
        .with_inter_threads(1);
    let detection = TextDetectionConfig {
        score_threshold: SCORE_THRESHOLD,
        box_threshold: BOX_THRESHOLD,
        unclip_ratio: UNCLIP_RATIO,
        max_candidates: 3000,
        ..TextDetectionConfig::default()
    };
    OAROCRBuilder::new(det, rec, dict)
        .text_detection_config(detection)
        .ort_session(session)
        .region_batch_size(16)
        .build()
        .map_err(|error| error.to_string())
}

fn recognize(engine: &OAROCR, rgb: &image::RgbImage) -> Result<(Vec<String>, f64), String> {
    let input = vec![rgb.clone()];
    let started = Instant::now();
    let results = engine.predict(input).map_err(|error| error.to_string())?;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let first = results
        .into_iter()
        .next()
        .ok_or_else(|| "the engine returned no result".to_string())?;
    let pieces = first
        .text_regions
        .iter()
        .filter_map(|region| region.text.as_deref().map(str::to_owned))
        .collect();
    Ok((pieces, elapsed_ms))
}

fn sha256(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn value(args: &[String], name: &str) -> Result<String, String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
        .ok_or_else(|| format!("the argument {name} is missing"))
}

fn run(args: &[String]) -> Result<Record, String> {
    let scale = value(args, "--scale")?;
    let target = value(args, "--target")?;
    let dylib = value(args, "--dylib")?;
    let det = value(args, "--det")?;
    let rec = value(args, "--rec")?;
    let dict = value(args, "--dict")?;
    let frame = Frame::load_png(Path::new(&value(args, "--frame")?))?;
    let image = if scale == "0.5" { frame.halve() } else { frame };
    let rgb = image::RgbImage::from_raw(image.width as u32, image.height as u32, image.rgb())
        .ok_or("the frame does not fill an image")?;
    let timer = Instant::now();
    let engine = load(
        Path::new(&dylib),
        Path::new(&det),
        Path::new(&rec),
        Path::new(&dict),
    )?;
    let load_ms = timer.elapsed().as_secs_f64() * 1000.0;
    let (_, first_ms) = recognize(&engine, &rgb)?;
    let cpu_start = sys::own_usage().cpu_ms;
    let (pieces, elapsed_ms) = recognize(&engine, &rgb)?;
    let cpu_ms = sys::own_usage().cpu_ms - cpu_start;
    Ok(extraction(
        &target,
        &pieces,
        elapsed_ms,
        cpu_ms,
        first_ms,
        (image.width, image.height),
    )
    .number("load_ms", How::Median, load_ms)
    .text("filter", if scale == "0.5" { "box-2x2" } else { "none" })
    .text("threads", &THREADS.to_string())
    .text("score_threshold", &SCORE_THRESHOLD.to_string())
    .text("box_threshold", &BOX_THRESHOLD.to_string())
    .text("unclip_ratio", &UNCLIP_RATIO.to_string())
    .text("runtime_sha256", &sha256(Path::new(&dylib))?)
    .text("det_sha256", &sha256(Path::new(&det))?)
    .text("rec_sha256", &sha256(Path::new(&rec))?)
    .text("dict_sha256", &sha256(Path::new(&dict))?))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    print_answer(run(&args));
}
