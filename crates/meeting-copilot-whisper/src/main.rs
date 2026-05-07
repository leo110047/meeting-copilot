use serde::{Deserialize, Serialize};
use std::env;
use std::io::{self, BufRead};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

const SAMPLE_RATE: u32 = 16_000;
const DEFAULT_MAX_NO_SPEECH_PROBABILITY: f32 = 0.65;
const DEFAULT_MIN_AVERAGE_TOKEN_PROBABILITY: f32 = 0.25;

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
        || error == "audio chunk is below speech energy threshold"
        || error == "Whisper classified the chunk as no speech"
        || error == "Whisper produced a low-confidence transcript"
}

fn whisper_thread_count() -> i32 {
    if let Ok(value) = env::var("MEETING_COPILOT_WHISPER_THREADS")
        && let Ok(parsed) = value.parse::<i32>()
    {
        return parsed.clamp(1, 4);
    }
    let available = std::thread::available_parallelism()
        .map(|count| count.get() as i32)
        .unwrap_or(2);
    available.clamp(1, 2)
}

fn audio_level(samples: &[f32]) -> (f32, f32) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }
    let mut sum_squares = 0.0_f32;
    let mut peak = 0.0_f32;
    for sample in samples {
        let magnitude = sample.abs();
        peak = peak.max(magnitude);
        sum_squares += sample * sample;
    }
    ((sum_squares / samples.len() as f32).sqrt(), peak)
}

fn chunk_looks_speech_like(level: (f32, f32)) -> bool {
    level.0 >= 0.0025 || level.1 >= 0.018
}

#[derive(Debug, Default)]
struct TranscriptQuality {
    token_probability_sum: f32,
    token_count: usize,
    max_no_speech_probability: f32,
}

#[derive(Debug, Clone, Copy)]
struct TranscriptQualityThresholds {
    max_no_speech_probability: f32,
    min_average_token_probability: f32,
}

impl Default for TranscriptQualityThresholds {
    fn default() -> Self {
        Self {
            max_no_speech_probability: DEFAULT_MAX_NO_SPEECH_PROBABILITY,
            min_average_token_probability: DEFAULT_MIN_AVERAGE_TOKEN_PROBABILITY,
        }
    }
}

fn transcript_quality_thresholds() -> TranscriptQualityThresholds {
    let max_no_speech = env::var("MEETING_COPILOT_WHISPER_MAX_NO_SPEECH_PROBABILITY").ok();
    let min_token = env::var("MEETING_COPILOT_WHISPER_MIN_TOKEN_PROBABILITY").ok();
    TranscriptQualityThresholds {
        max_no_speech_probability: probability_threshold(
            max_no_speech.as_deref(),
            DEFAULT_MAX_NO_SPEECH_PROBABILITY,
        ),
        min_average_token_probability: probability_threshold(
            min_token.as_deref(),
            DEFAULT_MIN_AVERAGE_TOKEN_PROBABILITY,
        ),
    }
}

fn probability_threshold(value: Option<&str>, default: f32) -> f32 {
    value
        .and_then(|item| item.parse::<f32>().ok())
        .filter(|item| item.is_finite())
        .map(|item| item.clamp(0.0, 1.0))
        .unwrap_or(default)
}

impl TranscriptQuality {
    fn observe_segment(&mut self, segment_no_speech_probability: f32) {
        self.max_no_speech_probability = self
            .max_no_speech_probability
            .max(segment_no_speech_probability);
    }

    fn observe_token(&mut self, token_probability: f32) {
        self.token_probability_sum += token_probability;
        self.token_count += 1;
    }

    fn average_token_probability(&self) -> Option<f32> {
        (self.token_count > 0).then(|| self.token_probability_sum / self.token_count as f32)
    }

    fn reject_reason(&self, thresholds: TranscriptQualityThresholds) -> Option<&'static str> {
        if self.max_no_speech_probability >= thresholds.max_no_speech_probability {
            return Some("Whisper classified the chunk as no speech");
        }
        if self
            .average_token_probability()
            .is_some_and(|value| value < thresholds.min_average_token_probability)
        {
            return Some("Whisper produced a low-confidence transcript");
        }
        None
    }
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
    let level = audio_level(&audio);
    if !chunk_looks_speech_like(level) {
        return Err("audio chunk is below speech energy threshold".to_string());
    }
    let mut state = ctx
        .create_state()
        .map_err(|error| format!("failed to create Whisper state: {error}"))?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_n_threads(whisper_thread_count());
    params.set_no_context(true);
    params.set_suppress_blank(true);
    params.set_suppress_nst(true);
    params.set_temperature(0.0);
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
    let mut quality = TranscriptQuality::default();
    for segment in state.as_iter() {
        quality.observe_segment(segment.no_speech_probability());
        for token_index in 0..segment.n_tokens() {
            if let Some(token) = segment.get_token(token_index) {
                quality.observe_token(token.token_probability());
            }
        }
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
    if let Some(reason) = quality.reject_reason(transcript_quality_thresholds()) {
        return Err(reason.to_string());
    }
    let inferred_end = started_at_ms
        + last_end
            .map(|centiseconds| i64::from(centiseconds) * 10)
            .unwrap_or_else(|| ((audio.len() as f64 / f64::from(SAMPLE_RATE)) * 1000.0) as i64);
    Ok(TranscriptLine {
        kind: "transcript",
        text,
        is_final: true,
        confidence: quality.average_token_probability().unwrap_or(0.0) as f64,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_energy_chunks_are_treated_as_empty() {
        assert!(!chunk_looks_speech_like(audio_level(&[0.0; 16_000])));
        assert!(chunk_looks_speech_like(audio_level(&[0.02; 16_000])));
        assert!(is_expected_empty_transcript(
            "audio chunk is below speech energy threshold"
        ));
    }

    #[test]
    fn no_speech_and_low_confidence_results_are_treated_as_empty() {
        let mut no_speech = TranscriptQuality::default();
        no_speech.observe_segment(0.7);
        no_speech.observe_token(0.9);
        assert_eq!(
            no_speech.reject_reason(TranscriptQualityThresholds::default()),
            Some("Whisper classified the chunk as no speech")
        );
        assert!(is_expected_empty_transcript(
            "Whisper classified the chunk as no speech"
        ));

        let mut low_confidence = TranscriptQuality::default();
        low_confidence.observe_segment(0.1);
        low_confidence.observe_token(0.1);
        assert_eq!(
            low_confidence.reject_reason(TranscriptQualityThresholds::default()),
            Some("Whisper produced a low-confidence transcript")
        );
    }

    #[test]
    fn quality_thresholds_are_explicit_and_tunable() {
        let mut quality = TranscriptQuality::default();
        quality.observe_segment(0.7);
        quality.observe_token(0.2);
        assert_eq!(
            quality.reject_reason(TranscriptQualityThresholds::default()),
            Some("Whisper classified the chunk as no speech")
        );
        assert_eq!(
            quality.reject_reason(TranscriptQualityThresholds {
                max_no_speech_probability: 0.8,
                min_average_token_probability: 0.1,
            }),
            None
        );
    }

    #[test]
    fn probability_threshold_parser_rejects_invalid_values_and_clamps_bounds() {
        assert_eq!(probability_threshold(Some("0.4"), 0.65), 0.4);
        assert_eq!(probability_threshold(Some("abc"), 0.65), 0.65);
        assert_eq!(probability_threshold(Some("NaN"), 0.65), 0.65);
        assert_eq!(probability_threshold(Some("2"), 0.65), 1.0);
        assert_eq!(probability_threshold(Some("-1"), 0.65), 0.0);
    }

    #[test]
    fn default_whisper_threads_are_bounded_for_live_capture() {
        let count = whisper_thread_count();
        assert!((1..=2).contains(&count));
    }
}
