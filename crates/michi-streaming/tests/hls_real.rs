use std::fs;
use std::io::Write;
use std::path::Path;

fn write_silent_wav(path: &Path, sample_rate: u32, seconds: u32) {
    let channels: u16 = 2;
    let bits_per_sample: u16 = 16;
    let bytes_per_sample = (bits_per_sample / 8) as u32;
    let samples = sample_rate * seconds;
    let data_size = samples * channels as u32 * bytes_per_sample;
    let byte_rate = sample_rate * channels as u32 * bytes_per_sample;
    let block_align = channels * (bits_per_sample / 8);

    let mut f = fs::File::create(path).unwrap();

    f.write_all(b"RIFF").unwrap();
    f.write_all(&(36u32 + data_size).to_le_bytes()).unwrap();
    f.write_all(b"WAVE").unwrap();
    f.write_all(b"fmt ").unwrap();
    f.write_all(&16u32.to_le_bytes()).unwrap();
    f.write_all(&1u16.to_le_bytes()).unwrap();
    f.write_all(&channels.to_le_bytes()).unwrap();
    f.write_all(&sample_rate.to_le_bytes()).unwrap();
    f.write_all(&byte_rate.to_le_bytes()).unwrap();
    f.write_all(&block_align.to_le_bytes()).unwrap();
    f.write_all(&bits_per_sample.to_le_bytes()).unwrap();
    f.write_all(b"data").unwrap();
    f.write_all(&data_size.to_le_bytes()).unwrap();

    let silence = vec![0u8; data_size as usize];
    f.write_all(&silence).unwrap();
}

#[tokio::test]
async fn hls_vod_generates_real_aac_48k_stereo() {
    assert!(
        michi_streaming::check_ffmpeg(),
        "ffmpeg is required for HLS certification"
    );

    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("source.wav");
    let cache = temp.path().join("cache");

    write_silent_wav(&input, 44_100, 3);

    michi_streaming::generate_hls_playlist(&input, &cache, "fixture-track")
        .await
        .expect("HLS generation failed");

    let playlist = michi_streaming::read_hls_playlist(&cache, "fixture-track")
        .await
        .expect("playlist missing");

    assert!(playlist.contains("#EXTM3U"));
    assert!(playlist.contains("#EXT-X-ENDLIST"));
    assert!(playlist.contains(".ts"));

    let first_segment = playlist
        .lines()
        .find(|line| line.ends_with(".ts"))
        .expect("no HLS segment found");

    let segment = michi_streaming::hls_segment_path(&cache, "fixture-track", first_segment);
    assert!(segment.exists());

    let output = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=codec_name,sample_rate,channels",
            "-of",
            "default=noprint_wrappers=1",
        ])
        .arg(&segment)
        .output()
        .expect("ffprobe failed to start");

    assert!(output.status.success());
    let probe = String::from_utf8_lossy(&output.stdout);

    assert!(probe.contains("codec_name=aac"), "{probe}");
    assert!(probe.contains("sample_rate=48000"), "{probe}");
    assert!(probe.contains("channels=2"), "{probe}");
}
