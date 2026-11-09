pub(super) struct Fixture {
    pub name: &'static str,
    pub max: f64,
    pub min: f64,
    pub silence_ms: u32,
    pub pcm: Vec<i16>,
}

pub(super) fn fixtures() -> Vec<Fixture> {
    let mut seed = 42u32;
    let mut make = |name, max, min, silence_ms, frames: usize, tail, quiet: fn(usize) -> bool| {
        let pcm = (0..frames * 320 + tail)
            .map(|i| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                if quiet(i / 320) {
                    0
                } else {
                    (seed >> 16) as i16
                }
            })
            .collect();
        Fixture {
            name,
            max,
            min,
            silence_ms,
            pcm,
        }
    };
    vec![
        make("default", 15.0, 8.0, 500, 2150, 113, |i| {
            i < 250 || (450..485).contains(&i) || i % 100 == 99
        }),
        make("retreat", 1.0, 5.0, 500, 270, 79, |i| {
            i == 30 || i == 87 || i % 50 == 24
        }),
        make("forced", 1.0, 5.0, 500, 211, 17, |_| false),
        make("silence", 1.0, 0.5, 100, 250, 237, |_| true),
        make("soft", 10.0, 1.0, 100, 300, 91, |i| i % 50 >= 45),
        make("subframe", 15.0, 8.0, 500, 0, 319, |_| false),
    ]
}
