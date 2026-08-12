use std::collections::BTreeMap;
use std::env;
use std::fmt::{self, Write as _};
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use bitb_rs::{BitBabbler, Fold};

const BAR_WIDTH: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Bits,
    U64,
    Range,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Hex,
    Binary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Config {
    mode: Mode,
    bits: usize,
    samples: usize,
    format: OutputFormat,
    start: u64,
    end: u64,
    fold: Fold,
    serial: Option<String>,
    show_bytes: usize,
    interval_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: Mode::Bits,
            bits: 8192,
            samples: 1,
            format: OutputFormat::Hex,
            start: 0,
            end: 10,
            fold: Fold::Raw,
            serial: None,
            show_bytes: 64,
            interval_ms: 0,
        }
    }
}

#[derive(Debug)]
struct CliError(String);

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CliError {}

#[derive(Debug)]
struct BitStats {
    zeros: u128,
    ones: u128,
    longest_zero_run: u64,
    longest_one_run: u64,
    current_bit: Option<bool>,
    current_run: u64,
    byte_frequencies: [u64; 256],
}

impl Default for BitStats {
    fn default() -> Self {
        Self {
            zeros: 0,
            ones: 0,
            longest_zero_run: 0,
            longest_one_run: 0,
            current_bit: None,
            current_run: 0,
            byte_frequencies: [0; 256],
        }
    }
}

impl BitStats {
    fn observe(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.byte_frequencies[usize::from(byte)] += 1;
            for bit_index in (0..8).rev() {
                let bit = byte & (1 << bit_index) != 0;
                if bit {
                    self.ones += 1;
                } else {
                    self.zeros += 1;
                }

                if self.current_bit == Some(bit) {
                    self.current_run += 1;
                } else {
                    self.current_bit = Some(bit);
                    self.current_run = 1;
                }

                if bit {
                    self.longest_one_run = self.longest_one_run.max(self.current_run);
                } else {
                    self.longest_zero_run = self.longest_zero_run.max(self.current_run);
                }
            }
        }
    }

    fn total_bits(&self) -> u128 {
        self.zeros + self.ones
    }
}

fn main() -> ExitCode {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return ExitCode::SUCCESS;
    }

    match parse_args(args).and_then(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            eprintln!("use --help to see the available options");
            ExitCode::FAILURE
        }
    }
}

fn parse_args(args: Vec<String>) -> Result<Config, CliError> {
    let mut config = Config::default();
    let mut fold_was_set = false;
    let mut args = args.into_iter();

    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| CliError(format!("missing value for {flag}")))?;

        match flag.as_str() {
            "--mode" => {
                config.mode = match value.as_str() {
                    "bits" => Mode::Bits,
                    "u64" => Mode::U64,
                    "range" => Mode::Range,
                    _ => return Err(CliError(format!("invalid mode: {value}"))),
                };
            }
            "--bits" => config.bits = parse_value(&flag, &value)?,
            "--samples" => config.samples = parse_value(&flag, &value)?,
            "--format" => {
                config.format = match value.as_str() {
                    "hex" => OutputFormat::Hex,
                    "binary" | "bin" => OutputFormat::Binary,
                    _ => return Err(CliError(format!("invalid format: {value}"))),
                };
            }
            "--start" => config.start = parse_value(&flag, &value)?,
            "--end" => config.end = parse_value(&flag, &value)?,
            "--fold" => {
                let fold: u8 = parse_value(&flag, &value)?;
                config.fold = Fold::try_from(fold).map_err(|error| CliError(error.to_string()))?;
                fold_was_set = true;
            }
            "--serial" => {
                if value.is_empty() {
                    return Err(CliError("--serial must not be empty".into()));
                }
                config.serial = Some(value);
            }
            "--show-bytes" => config.show_bytes = parse_value(&flag, &value)?,
            "--interval-ms" => config.interval_ms = parse_value(&flag, &value)?,
            _ => return Err(CliError(format!("unknown option: {flag}"))),
        }
    }

    if config.samples == 0 {
        return Err(CliError("--samples must be greater than zero".into()));
    }
    if config.mode == Mode::Bits && (config.bits == 0 || config.bits % 8 != 0) {
        return Err(CliError(
            "--bits must be greater than zero and divisible by 8".into(),
        ));
    }
    if config.mode == Mode::Range && config.start >= config.end {
        return Err(CliError("--start must be less than --end".into()));
    }
    if fold_was_set && config.mode != Mode::Bits {
        return Err(CliError("--fold is available only in bits mode".into()));
    }

    Ok(config)
}

fn parse_value<T>(flag: &str, value: &str) -> Result<T, CliError>
where
    T: std::str::FromStr,
{
    value
        .parse()
        .map_err(|_| CliError(format!("invalid value for {flag}: {value}")))
}

fn run(config: Config) -> Result<(), CliError> {
    let mut device = match config.serial.as_deref() {
        Some(serial) => BitBabbler::open_by_serial(serial),
        None => BitBabbler::open(),
    }
    .map_err(|error| CliError(format!("failed to open BitBabbler: {error}")))?;

    print_header(&device, &config);
    let started = Instant::now();

    match config.mode {
        Mode::Bits => run_bits(&mut device, &config, started),
        Mode::U64 => run_u64(&mut device, &config, started),
        Mode::Range => run_range(&mut device, &config, started),
    }
}

fn run_bits(device: &mut BitBabbler, config: &Config, started: Instant) -> Result<(), CliError> {
    let mut stats = BitStats::default();

    for sample in 1..=config.samples {
        let bytes = if config.fold == Fold::Raw {
            device.get_bits(config.bits)
        } else {
            device.get_bits_with_fold(config.bits, config.fold)
        }
        .map_err(|error| CliError(format!("sample {sample} failed: {error}")))?;

        stats.observe(&bytes);
        println!(
            "sample {sample:>4}: {}",
            preview(&bytes, config.format, config.show_bytes)
        );
        wait_between_samples(sample, config);
    }

    print_bit_stats(
        &stats,
        started.elapsed(),
        config.fold.segment_count() as u128,
    );
    Ok(())
}

fn run_u64(device: &mut BitBabbler, config: &Config, started: Instant) -> Result<(), CliError> {
    let mut stats = BitStats::default();

    for sample in 1..=config.samples {
        let value = device
            .random_u64()
            .map_err(|error| CliError(format!("sample {sample} failed: {error}")))?;
        stats.observe(&value.to_le_bytes());
        println!("sample {sample:>4}: {value:>20}  0x{value:016x}");
        wait_between_samples(sample, config);
    }

    print_bit_stats(&stats, started.elapsed(), 1);
    Ok(())
}

fn run_range(device: &mut BitBabbler, config: &Config, started: Instant) -> Result<(), CliError> {
    let mut counts = BTreeMap::<u64, u64>::new();

    for sample in 1..=config.samples {
        let value = device
            .random_range(config.start..config.end)
            .map_err(|error| CliError(format!("sample {sample} failed: {error}")))?;
        *counts.entry(value).or_default() += 1;
        wait_between_samples(sample, config);
    }

    print_range_histogram(config, &counts, started.elapsed());
    Ok(())
}

fn wait_between_samples(sample: usize, config: &Config) {
    if sample < config.samples && config.interval_ms > 0 {
        thread::sleep(Duration::from_millis(config.interval_ms));
    }
}

fn print_header(device: &BitBabbler, config: &Config) {
    let mode = match config.mode {
        Mode::Bits => "bits",
        Mode::U64 => "u64",
        Mode::Range => "range",
    };
    let info = device.device_info();

    println!("BitBabbler laboratory");
    println!("variant          : {:?}", info.variant);
    println!("product          : {}", info.product);
    println!("serial           : {}", info.serial);
    println!("mode             : {mode}");
    println!("samples          : {}", config.samples);
    if config.mode == Mode::Bits {
        println!("bits per sample  : {}", config.bits);
        println!("fold             : {}", config.fold.as_u8());
        println!("raw segments     : {}", config.fold.segment_count());
    }
    if config.mode == Mode::Range {
        println!("range            : [{}..{})", config.start, config.end);
    }
    println!();
}

fn print_bit_stats(stats: &BitStats, elapsed: Duration, traffic_factor: u128) {
    let total = stats.total_bits();
    let zero_percent = percentage(stats.zeros, total);
    let one_percent = percentage(stats.ones, total);
    let seconds = elapsed.as_secs_f64();
    let output_throughput = throughput_mbit(total, seconds);
    let estimated_raw_bits = total.saturating_mul(traffic_factor);
    let raw_throughput = throughput_mbit(estimated_raw_bits, seconds);

    println!();
    println!("descriptive summary (never gates output)");
    println!("output bits      : {total}");
    println!("raw device bits  : {estimated_raw_bits}");
    println!("elapsed          : {:.3} ms", seconds * 1_000.0);
    println!("output throughput: {output_throughput:.3} Mbit/s");
    println!("raw throughput   : {raw_throughput:.3} Mbit/s");
    println!(
        "zeros            : {} ({zero_percent:.4}%) {}",
        stats.zeros,
        bar(stats.zeros, total)
    );
    println!(
        "ones             : {} ({one_percent:.4}%) {}",
        stats.ones,
        bar(stats.ones, total)
    );
    println!("longest zero run : {}", stats.longest_zero_run);
    println!("longest one run  : {}", stats.longest_one_run);
    println!("most common bytes: {}", common_bytes(stats));
}

fn throughput_mbit(bits: u128, seconds: f64) -> f64 {
    if seconds > 0.0 {
        bits as f64 / seconds / 1_000_000.0
    } else {
        0.0
    }
}

fn print_range_histogram(config: &Config, counts: &BTreeMap<u64, u64>, elapsed: Duration) {
    let max_count = counts.values().copied().max().unwrap_or(0);
    let width = config.end - config.start;
    let show_empty_buckets = width <= 256;

    println!("descriptive histogram (never gates output)");
    if show_empty_buckets {
        for value in config.start..config.end {
            print_bucket(
                value,
                counts.get(&value).copied().unwrap_or(0),
                max_count,
                config.samples,
            );
        }
    } else {
        println!("range has {width} values; showing only values observed in the samples");
        for (&value, &count) in counts {
            print_bucket(value, count, max_count, config.samples);
        }
    }
    println!();
    println!(
        "elapsed          : {:.3} ms",
        elapsed.as_secs_f64() * 1_000.0
    );
    println!("distinct values  : {}", counts.len());
}

fn print_bucket(value: u64, count: u64, max_count: u64, samples: usize) {
    let bar_length = if max_count == 0 {
        0
    } else {
        ((u128::from(count) * BAR_WIDTH as u128) / u128::from(max_count)) as usize
    };
    let percent = percentage(u128::from(count), samples as u128);
    println!(
        "{value:>20} | {:<BAR_WIDTH$} {count:>8} ({percent:>7.3}%)",
        "#".repeat(bar_length)
    );
}

fn preview(bytes: &[u8], format: OutputFormat, show_bytes: usize) -> String {
    if show_bytes == 0 {
        return "(output hidden)".into();
    }

    if bytes.len() <= show_bytes {
        return format_bytes(bytes, format);
    }

    let head_len = show_bytes.div_ceil(2);
    let tail_len = show_bytes / 2;
    format!(
        "{} ... {} ({} bytes total)",
        format_bytes(&bytes[..head_len], format),
        format_bytes(&bytes[bytes.len() - tail_len..], format),
        bytes.len()
    )
}

fn format_bytes(bytes: &[u8], format: OutputFormat) -> String {
    let mut output = String::new();
    for (index, byte) in bytes.iter().enumerate() {
        if index > 0 {
            output.push(' ');
        }
        match format {
            OutputFormat::Hex => write!(&mut output, "{byte:02x}"),
            OutputFormat::Binary => write!(&mut output, "{byte:08b}"),
        }
        .expect("writing to a String cannot fail");
    }
    output
}

fn percentage(part: u128, total: u128) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 * 100.0 / total as f64
    }
}

fn bar(part: u128, total: u128) -> String {
    if total == 0 {
        return String::new();
    }
    let length = ((part * BAR_WIDTH as u128) / total) as usize;
    "#".repeat(length)
}

fn common_bytes(stats: &BitStats) -> String {
    let mut frequencies = stats
        .byte_frequencies
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, count)| *count > 0)
        .collect::<Vec<_>>();
    frequencies.sort_unstable_by(|(byte_a, count_a), (byte_b, count_b)| {
        count_b.cmp(count_a).then_with(|| byte_a.cmp(byte_b))
    });

    frequencies
        .into_iter()
        .take(8)
        .map(|(byte, count)| format!("{byte:02x}={count}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn print_help() {
    println!(
        "\
Visual laboratory for bitb-rs (BitBabbler White/Black)

USAGE:
    cargo run --release --example bitb_lab -- [OPTIONS]

OPTIONS:
    --mode bits|u64|range       Operation to exercise (default: bits)
    --bits N                    Bits per sample; positive and byte-aligned (default: 8192)
    --samples N                 Number of samples (default: 1)
    --format hex|binary         Buffer preview format (default: hex)
    --fold 0|1|2|3|4            XOR fold for bits mode only (default: 0/raw)
    --serial VALUE              Open the device with this exact serial
    --start N                   Inclusive range start (default: 0)
    --end N                     Exclusive range end (default: 10)
    --show-bytes N              Bytes shown from each buffer; 0 hides data (default: 64)
    --interval-ms N             Delay between samples (default: 0)
    -h, --help                  Show this help

EXAMPLES:
    cargo run --release --example bitb_lab -- --bits 8192 --samples 10
    cargo run --release --example bitb_lab -- --bits 64 --fold 2 --format binary
    cargo run --release --example bitb_lab -- --mode u64 --samples 20
    cargo run --release --example bitb_lab -- --mode range --start 0 --end 10 --samples 10000
    cargo run --release --example bitb_lab -- --serial YOUR_SERIAL --fold 1"
    );
}

#[cfg(test)]
mod tests {
    use super::{BitStats, Config, Fold, Mode, OutputFormat, parse_args, preview};

    #[test]
    fn defaults_are_raw_and_suitable_for_a_quick_run() {
        let config = parse_args(Vec::new()).expect("defaults");
        assert_eq!(config, Config::default());
        assert_eq!(config.bits, 8192);
        assert_eq!(config.samples, 1);
        assert_eq!(config.fold, Fold::Raw);
        assert_eq!(config.serial, None);
    }

    #[test]
    fn parses_fold_and_serial_for_bits() {
        let config = parse_args(
            ["--bits", "64", "--fold", "4", "--serial", "DEVICE"]
                .map(String::from)
                .to_vec(),
        )
        .expect("valid options");
        assert_eq!(config.bits, 64);
        assert_eq!(config.fold, Fold::Four);
        assert_eq!(config.serial.as_deref(), Some("DEVICE"));
    }

    #[test]
    fn parses_range_configuration() {
        let config = parse_args(
            [
                "--mode",
                "range",
                "--start",
                "10",
                "--end",
                "20",
                "--samples",
                "500",
            ]
            .map(String::from)
            .to_vec(),
        )
        .expect("valid options");
        assert_eq!(config.mode, Mode::Range);
        assert_eq!(config.start, 10);
        assert_eq!(config.end, 20);
        assert_eq!(config.samples, 500);
    }

    #[test]
    fn rejects_invalid_fold_and_fold_outside_bits_mode() {
        let invalid = parse_args(["--fold", "5"].map(String::from).to_vec()).unwrap_err();
        assert!(invalid.to_string().contains("between 0 and 4"));

        let wrong_mode =
            parse_args(["--mode", "u64", "--fold", "0"].map(String::from).to_vec()).unwrap_err();
        assert!(wrong_mode.to_string().contains("only in bits mode"));
    }

    #[test]
    fn rejects_invalid_bit_size_and_empty_serial() {
        let bits = parse_args(["--bits", "7"].map(String::from).to_vec()).unwrap_err();
        assert!(bits.to_string().contains("divisible by 8"));

        let serial = parse_args(["--serial", ""].map(String::from).to_vec()).unwrap_err();
        assert!(serial.to_string().contains("must not be empty"));
    }

    #[test]
    fn formats_complete_and_truncated_previews() {
        let bytes = [0x01, 0xab, 0xff, 0x10];
        assert_eq!(preview(&bytes, OutputFormat::Hex, 4), "01 ab ff 10");
        assert_eq!(
            preview(&bytes, OutputFormat::Binary, 2),
            "00000001 ... 00010000 (4 bytes total)"
        );
    }

    #[test]
    fn calculates_bit_counts_and_runs() {
        let mut stats = BitStats::default();
        stats.observe(&[0b1111_0000, 0b0000_0001]);
        assert_eq!(stats.ones, 5);
        assert_eq!(stats.zeros, 11);
        assert_eq!(stats.longest_one_run, 4);
        assert_eq!(stats.longest_zero_run, 11);
    }
}
