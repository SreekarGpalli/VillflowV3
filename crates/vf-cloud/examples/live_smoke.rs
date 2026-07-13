//! Live smoke test for VillFlow cloud clients — requires real API keys in
//! `%APPDATA%\VillFlow\settings.json` and network access.
//!
//! Usage: `cargo run -p vf-cloud --example live_smoke -- [path\to\16k_mono_s16le.wav]`
//! Without a wav argument only the Groq checks run.

use std::time::Instant;

fn read_settings() -> vf_core::Settings {
    let appdata = std::env::var("APPDATA").expect("APPDATA not set");
    let path = std::path::Path::new(&appdata)
        .join("VillFlow")
        .join("settings.json");
    let raw = std::fs::read_to_string(&path)
        .expect("settings.json missing — run the app once or create it");
    serde_json::from_str(&raw).expect("settings.json parse failed")
}

/// Minimal WAV reader: returns the PCM bytes of the `data` chunk.
/// Assumes the file is already 16 kHz mono s16le (as produced for this test).
fn read_wav_data(path: &str) -> Vec<u8> {
    let bytes = std::fs::read(path).expect("wav read failed");
    assert!(
        bytes.len() > 44 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE",
        "not a WAV file"
    );
    let mut i = 12usize;
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let size = u32::from_le_bytes([bytes[i + 4], bytes[i + 5], bytes[i + 6], bytes[i + 7]]) as usize;
        if id == b"data" {
            let end = (i + 8 + size).min(bytes.len());
            return bytes[i + 8..end].to_vec();
        }
        i += 8 + size + (size & 1);
    }
    panic!("no data chunk found");
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let settings = read_settings();

    // --- 1. Groq ---
    println!("[groq] listing models…");
    let t = Instant::now();
    match vf_cloud::list_models(&settings.llm.api_key).await {
        Ok(models) => println!(
            "[groq] OK — {} models in {:?}; configured model '{}' present: {}",
            models.len(),
            t.elapsed(),
            settings.llm.model,
            models.contains(&settings.llm.model)
        ),
        Err(e) => println!("[groq] LIST FAILED: {e}"),
    }

    let t = Instant::now();
    match vf_cloud::chat_completion(
        "You are a connectivity test. Reply with exactly: OK",
        "ping",
        &settings.llm.model,
        &settings.llm.api_key,
    )
    .await
    {
        Ok(reply) => println!("[groq] chat OK in {:?} — reply: {reply}", t.elapsed()),
        Err(e) => println!("[groq] CHAT FAILED: {e}"),
    }

    // --- 2. ElevenLabs realtime STT ---
    let Some(wav) = std::env::args().nth(1) else {
        println!("[stt] no wav path given — skipping STT test");
        return;
    };
    let pcm = read_wav_data(&wav);
    println!(
        "[stt] {} bytes PCM ({:.1}s) — opening session…",
        pcm.len(),
        pcm.len() as f64 / 32000.0
    );

    let t = Instant::now();
    match vf_cloud::SttSession::open(settings.stt.clone(), vec![]).await {
        Ok(session) => {
            println!("[stt] session open in {:?}", t.elapsed());
            let mut partials = session.subscribe_partials();
            tokio::spawn(async move {
                while let Ok(p) = partials.recv().await {
                    println!("[stt]   partial: {p}");
                }
            });
            // Stream in 100ms chunks, faster than realtime.
            for chunk in pcm.chunks(3200) {
                if let Err(e) = session.feed_pcm(chunk).await {
                    println!("[stt] FEED FAILED: {e}");
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
            let t2 = Instant::now();
            match session.commit().await {
                Ok(transcript) => println!(
                    "[stt] committed in {:?} — transcript: {transcript}",
                    t2.elapsed()
                ),
                Err(e) => println!("[stt] COMMIT FAILED: {e}"),
            }
        }
        Err(e) => println!("[stt] OPEN FAILED: {e}"),
    }
}
