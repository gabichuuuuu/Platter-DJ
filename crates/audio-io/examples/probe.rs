//! Opens the default output device, plays silence for a moment and prints what the stream
//! runs at. A manual check of the device backend: the tests never open a real device.
//!
//! `cargo run -p platter-audio-io --example probe`

use std::time::Duration;

use platter_audio_io::{Routing, Source, StreamRequest, devices, prepare};

fn main() {
    for device in devices() {
        println!(
            "device: kind {:?}, {} outputs, {} inputs, rates {:?}, default {}",
            device.kind, device.outputs, device.inputs, device.sample_rates, device.is_default
        );
    }
    let request = StreamRequest {
        device: None,
        sample_rate: 48_000,
        buffer_frames: 256,
        routing: Routing::default(),
    };
    let silence: Source = Box::new(|output, _| output.fill(0.0));
    let started = prepare(&request)
        .and_then(|prepared| prepared.start(silence).map_err(|failed| failed.error));
    let output = match started {
        Ok(output) => output,
        Err(error) => {
            println!("no stream: {error}");
            return;
        }
    };
    std::thread::sleep(Duration::from_millis(500));
    let info = output.info();
    println!(
        "stream: {} Hz, {} channels, {} frames per buffer ({:.1} ms), headphones {}",
        info.sample_rate,
        info.channels,
        info.buffer_frames,
        info.latency_ms(),
        info.headphones
    );
    println!(
        "load {:.1} %, dropouts {}",
        output.stats().load() * 100.0,
        output.stats().dropouts()
    );
    println!("source returned: {}", output.stop().is_some());
}
