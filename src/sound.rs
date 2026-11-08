//! Little synthesized sound effects (no audio files). The audio device opens on first use.

use rodio::source::{SawtoothWave, SineWave, Source, SquareWave, TriangleWave};
use std::time::Duration;

pub struct Sound {
    pub enabled: bool,
    sink: Option<rodio::MixerDeviceSink>,
    failed: bool,
}

impl Sound {
    pub fn new(enabled: bool) -> Sound {
        Sound { enabled, sink: None, failed: false }
    }

    fn tone(&self, wave: &str, freq: f32, start_ms: u64, dur_ms: u64, vol: f32) {
        let Some(sink) = &self.sink else { return };
        let d = Duration::from_millis(dur_ms);
        let st = Duration::from_millis(start_ms);
        let fade = Duration::from_millis(15);
        let m = sink.mixer();
        match wave {
            "triangle" => m.add(TriangleWave::new(freq).take_duration(d).fade_in(fade).fade_out(d).amplify(vol).delay(st)),
            "square" => m.add(SquareWave::new(freq).take_duration(d).fade_in(fade).fade_out(d).amplify(vol * 0.5).delay(st)),
            "saw" => m.add(SawtoothWave::new(freq).take_duration(d).fade_in(fade).fade_out(d).amplify(vol * 0.5).delay(st)),
            _ => m.add(SineWave::new(freq).take_duration(d).fade_in(fade).fade_out(d).amplify(vol).delay(st)),
        }
    }

    pub fn play(&mut self, name: &str) {
        self.play_n(name, 0);
    }

    pub fn play_n(&mut self, name: &str, n: i64) {
        if !self.enabled || self.failed {
            return;
        }
        if self.sink.is_none() {
            match rodio::DeviceSinkBuilder::open_default_sink() {
                Ok(mut s) => {
                    s.log_on_drop(false);
                    self.sink = Some(s)
                }
                Err(_) => {
                    self.failed = true; // no audio device: stay silent
                    return;
                }
            }
        }
        match name {
            "correct" => {
                self.tone("triangle", 660.0, 0, 150, 0.25);
                self.tone("triangle", 990.0, 90, 250, 0.25);
            }
            "wrong" => {
                self.tone("saw", 220.0, 0, 180, 0.15);
                self.tone("saw", 180.0, 120, 250, 0.15);
            }
            "tap" => self.tone("sine", 520.0, 0, 50, 0.08),
            "combo" => {
                let b = 520.0 + n.min(12) as f32 * 40.0;
                self.tone("triangle", b, 0, 100, 0.25);
                self.tone("triangle", b * 1.5, 60, 180, 0.25);
            }
            "complete" => {
                for (i, f) in [523.0, 659.0, 784.0, 1047.0].iter().enumerate() {
                    self.tone("triangle", *f, i as u64 * 110, 350, 0.22);
                }
            }
            "levelup" => {
                for (i, f) in [392.0, 523.0, 659.0, 784.0, 1047.0, 1319.0].iter().enumerate() {
                    self.tone("square", *f, i as u64 * 80, 400, 0.12);
                }
            }
            "achievement" => {
                for (i, f) in [784.0, 988.0, 1175.0].iter().enumerate() {
                    self.tone("sine", *f, i as u64 * 100, 450, 0.22);
                }
            }
            _ => {}
        }
    }
}
