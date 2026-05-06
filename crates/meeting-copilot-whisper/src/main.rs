use serde::{Deserialize, Serialize};
use std::env;
use std::io::{self, BufRead};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

const SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, Default)]
struct CliOptions {
    health: bool,
    serve: bool,
    model: Option<PathBuf>,
    file: Option<PathBuf>,
    language: String,
    source: String,
    started_at_ms: i64,
    ended_at_ms: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptionJob {
    file: PathBuf,
    language: String,
    source: String,
    route: Option<String>,
    started_at_ms: i64,
    ended_at_ms: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthLine {
    kind: &'static str,
    provider_id: &'static str,
    ready: bool,
    supports_streaming: bool,
    supports_diarization: bool,
    supports_source_hints: bool,
    last_error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptLine {
    kind: &'static str,
    text: String,
    is_final: bool,
    confidence: f64,
    language: String,
    source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    route: Option<String>,
    started_at_ms: i64,
    ended_at_ms: i64,
}

fn main() {
    let options = match parse_args(env::args().skip(1)) {
        Ok(options) => options,
        Err(error) => exit_with_error(error),
    };
    if options.health {
        let health = health_line(options.model.as_deref());
        write_json(&health);
        std::process::exit(if health.ready { 0 } else { 2 });
    }
    if options.serve {
        if let Err(error) = run_server(options) {
            exit_with_error(error);
        }
        return;
    }
    match run_transcription(options) {
        Ok(line) => write_json(&line),
        Err(error) => exit_with_error(error),
    }
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<CliOptions, String> {
    let mut options = CliOptions {
        language: "zh-TW".to_string(),
        source: "mic".to_string(),
        ..CliOptions::default()
    };
    let values: Vec<String> = args.collect();
    let mut index = 0;
    while index < values.len() {
        match values[index].as_str() {
            "--health" => options.health = true,
            "--serve" => options.serve = true,
            "--model" => {
                index += 1;
                options.model = values.get(index).map(PathBuf::from);
            }
            "--file" => {
                index += 1;
                options.file = values.get(index).map(PathBuf::from);
            }
            "--language" => {
                index += 1;
                if let Some(value) = values.get(index) {
                    options.language = normalize_language(value);
                }
            }
            "--source" => {
                index += 1;
                if let Some(value) = values.get(index) {
                    options.source = normalize_source(value);
                }
            }
            "--started-at-ms" => {
                index += 1;
                options.started_at_ms = values
                    .get(index)
                    .and_then(|value| value.parse::<i64>().ok())
                    .unwrap_or(0);
            }
            "--ended-at-ms" => {
                index += 1;
                options.ended_at_ms = values
                    .get(index)
                    .and_then(|value| value.parse::<i64>().ok());
            }
            value => return Err(format!("unknown argument: {value}")),
        }
        index += 1;
    }
    Ok(options)
}

fn run_server(options: CliOptions) -> Result<(), String> {
    let model = options
        .model
        .ok_or_else(|| "--model is required".to_string())?;
    let ctx = WhisperContext::new_with_params(
        model.to_string_lossy().as_ref(),
        WhisperContextParameters::default(),
    )
    .map_err(|error| format!("failed to load Whisper model: {error}"))?;
    for line in io::stdin().lock().lines() {
        let line = line.map_err(|error| format!("failed to read server input: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let job: TranscriptionJob = serde_json::from_str(&line)
            .map_err(|error| format!("invalid transcription job: {error}"))?;
        let result = catch_unwind(AssertUnwindSafe(|| {
            transcribe_file_with_context(
                &ctx,
                &job.file,
                &job.language,
                &job.source,
                job.route,
                job.started_at_ms,
                job.ended_at_ms,
            )
        }))
        .map_err(|_| "Whisper transcription panicked".to_string())
        .and_then(|result| result);
        match result {
            Ok(line) => write_json(&line),
            Err(error) if is_expected_empty_transcript(&error) => {}
            Err(error) => eprintln!("{error}"),
        }
        let _ = std::fs::remove_file(&job.file);
    }
    Ok(())
}

fn is_expected_empty_transcript(error: &str) -> bool {
    error == "Whisper produced no transcript"
        || error == "audio chunk is too short for Whisper transcription"
}

fn health_line(model: Option<&Path>) -> HealthLine {
    let last_error = match model {
        Some(path) if path.exists() => match WhisperContext::new_with_params(
            path.to_string_lossy().as_ref(),
            WhisperContextParameters::default(),
        ) {
            Ok(_) => None,
            Err(error) => Some(format!("failed to load Whisper model: {error}")),
        },
        Some(path) => Some(format!("Whisper model not found: {}", path.display())),
        None => Some("missing --model".to_string()),
    };
    HealthLine {
        kind: "health",
        provider_id: "local-whisper",
        ready: last_error.is_none(),
        supports_streaming: true,
        supports_diarization: false,
        supports_source_hints: true,
        last_error,
    }
}

fn run_transcription(options: CliOptions) -> Result<TranscriptLine, String> {
    let model = options
        .model
        .ok_or_else(|| "--model is required".to_string())?;
    let file = options
        .file
        .ok_or_else(|| "--file is required".to_string())?;
    let ctx = WhisperContext::new_with_params(
        model.to_string_lossy().as_ref(),
        WhisperContextParameters::default(),
    )
    .map_err(|error| format!("failed to load Whisper model: {error}"))?;
    transcribe_file_with_context(
        &ctx,
        &file,
        &options.language,
        &options.source,
        None,
        options.started_at_ms,
        options.ended_at_ms,
    )
}

fn transcribe_file_with_context(
    ctx: &WhisperContext,
    file: &Path,
    language: &str,
    source: &str,
    route: Option<String>,
    started_at_ms: i64,
    ended_at_ms: Option<i64>,
) -> Result<TranscriptLine, String> {
    let audio = read_wav_as_16k_mono_f32(file)?;
    if audio.len() < SAMPLE_RATE as usize / 4 {
        return Err("audio chunk is too short for Whisper transcription".to_string());
    }
    let mut state = ctx
        .create_state()
        .map_err(|error| format!("failed to create Whisper state: {error}"))?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_translate(false);
    params.set_language(Some(whisper_language(language)));
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    state
        .full(params, &audio)
        .map_err(|error| format!("failed to run Whisper: {error}"))?;
    let mut text = String::new();
    let mut last_end = None;
    for segment in state.as_iter() {
        let segment_text = segment
            .to_str_lossy()
            .map_err(|error| format!("failed to read Whisper segment: {error}"))?;
        let trimmed = segment_text.trim();
        if !trimmed.is_empty() {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(trimmed);
        }
        last_end = Some(segment.end_timestamp());
    }
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("Whisper produced no transcript".to_string());
    }
    let inferred_end = started_at_ms
        + last_end
            .map(|centiseconds| i64::from(centiseconds) * 10)
            .unwrap_or_else(|| ((audio.len() as f64 / f64::from(SAMPLE_RATE)) * 1000.0) as i64);
    Ok(TranscriptLine {
        kind: "transcript",
        text,
        is_final: true,
        // whisper-rs does not expose a stable utterance-level confidence here.
        confidence: 0.72,
        language: language.to_string(),
        source: source.to_string(),
        route,
        started_at_ms,
        ended_at_ms: ended_at_ms.unwrap_or(inferred_end),
    })
}

fn read_wav_as_16k_mono_f32(path: &Path) -> Result<Vec<f32>, String> {
    let mut reader =
        hound::WavReader::open(path).map_err(|error| format!("failed to open wav: {error}"))?;
    let spec = reader.spec();
    let channels = usize::from(spec.channels.max(1));
    let samples = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to read float wav samples: {error}"))?,
        hound::SampleFormat::Int => {
            let max = ((1_i64 << (u32::from(spec.bits_per_sample).saturating_sub(1))) - 1) as f32;
            if spec.bits_per_sample <= 16 {
                reader
                    .samples::<i16>()
                    .map(|sample| sample.map(|value| f32::from(value) / max))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| format!("failed to read int16 wav samples: {error}"))?
            } else {
                reader
                    .samples::<i32>()
                    .map(|sample| sample.map(|value| value as f32 / max))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| format!("failed to read int32 wav samples: {error}"))?
            }
        }
    };
    let mono = if channels == 1 {
        samples
    } else {
        samples
            .chunks(channels)
            .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
            .collect()
    };
    Ok(resample_linear(&mono, spec.sample_rate, SAMPLE_RATE))
}

fn resample_linear(samples: &[f32], input_rate: u32, output_rate: u32) -> Vec<f32> {
    if samples.is_empty() || input_rate == output_rate {
        return samples.to_vec();
    }
    let output_len =
        ((samples.len() as f64) * f64::from(output_rate) / f64::from(input_rate)).round() as usize;
    let ratio = f64::from(input_rate) / f64::from(output_rate);
    (0..output_len)
        .map(|index| {
            let position = index as f64 * ratio;
            let lower = position.floor() as usize;
            let upper = lower.saturating_add(1).min(samples.len().saturating_sub(1));
            let fraction = (position - lower as f64) as f32;
            samples[lower] * (1.0 - fraction) + samples[upper] * fraction
        })
        .collect()
}

fn normalize_language(value: &str) -> String {
    match value {
        "zh-TW" | "zh-CN" => "zh".to_string(),
        "en-US" | "en-GB" => "en".to_string(),
        "ja-JP" => "ja".to_string(),
        "ko-KR" => "ko".to_string(),
        _ => value.to_string(),
    }
}

fn whisper_language(value: &str) -> &str {
    match value {
        "zh-TW" | "zh-CN" => "zh",
        "en-US" | "en-GB" => "en",
        "ja-JP" => "ja",
        "ko-KR" => "ko",
        _ => value,
    }
}

fn normalize_source(value: &str) -> String {
    match value {
        "mic" | "system" => value.to_string(),
        _ => "mic".to_string(),
    }
}

fn write_json<T: Serialize>(value: &T) {
    match serde_json::to_string(value) {
        Ok(line) => println!("{line}"),
        Err(error) => exit_with_error(format!("failed to encode JSON: {error}")),
    }
}

fn exit_with_error(message: String) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}
