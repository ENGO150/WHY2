/*
This is part of WHY2
Copyright (C) 2022-2026 Václav Šmejkal

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

use std::
{
    ops::RangeInclusive,
    collections::VecDeque,
    sync::
    {
        Mutex,
        atomic::{ AtomicBool, AtomicU32, AtomicUsize, Ordering },
    },
};

use ringbuf::
{
    HeapRb,
    HeapCons,
    HeapProd,
    traits::
    {
        Split,
        Producer,
        Consumer,
    },
};

use crate::network::voice::consts;

//STRUCTS
enum State
{
    Searching, //THE DELAY IS NOT KNOWN YET
    Locked,    //THE FILTER IS RUNNING
}

//THE CAPTURE'S END OF THE TAP
pub struct Canceller
{
    consumer: HeapCons<f32>,
    state: State,

    //REFERENCE RESAMPLER (RATE 0 = NO VOICE OUTPUT)
    rate: u32,
    step: f32,
    position: f32,
    current: f32,
    next: f32,

    //HISTORY, NEWEST AT THE BACK; ALSO THE DELAY LINE
    reference: VecDeque<f32>,
    capture: VecDeque<f32>,

    //FILTER
    weights: Vec<f32>,
    best: Vec<f32>,   //THE BEST FILTER THIS LOCK HAS MANAGED
    best_erle: f32,   //WHAT IT SCORED
    failures: usize,  //SCORING WINDOWS PUTTING IT BACK HAS NOT RESCUED
    offset: usize,      //HOW FAR BACK THE FIRST TAP SITS
    norm: f32,          //ENERGY OF THE TAP WINDOW
    norm_taps: usize,   //HOW MANY TAPS THAT WAS SUMMED OVER
    capture_power: f32, //MEAN SQUARE OF THE CAPTURE

    //GUARDS
    countdown: usize,     //SAMPLES LEFT BEFORE THE NEXT SEARCH IS WORTH ATTEMPTING
    scored: usize,        //SAMPLES IN THE CURRENT ERLE WINDOW
    capture_energy: f32,  //ENERGY THAT WENT INTO IT
    residual_energy: f32, //ENERGY THAT CAME OUT

    //REFERENCE SAMPLES THE RING COULD NOT SUPPLY
    phantoms: usize,

    //THE GAIN THE SEARCH FOUND
    gain: f32,
}

//GLOBAL VARIABLES
static REFERENCE: Mutex<Option<HeapProd<f32>>> = Mutex::new(None); //THE VOICE OUTPUT CALLBACK'S END OF THE TAP
static ACTIVE: AtomicBool = AtomicBool::new(false);                //IS ANYBODY SHARING?
static DESYNC: AtomicBool = AtomicBool::new(false);                //THE REFERENCE LOST SAMPLES - ALIGNMENT IS GONE
static RATE: AtomicU32 = AtomicU32::new(0);                        //SAMPLE RATE OF THE VOICE OUTPUT STREAM
static SKIPPED: AtomicUsize = AtomicUsize::new(0);                 //CAPTURED FRAMES THAT NEVER REACHED US

//IMPLEMENTATIONS
impl Drop for Canceller
{
    fn drop(&mut self)
    {
        stop();
    }
}

impl Canceller
{
    //CANCEL ONE CAPTURED CHUNK IN PLACE
    pub fn process(&mut self, chunk: &mut [f32])
    {
        self.follow_output_stream();

        //CAPTURED FRAMES THE CHANNEL COULD NOT HOLD
        let skipped = SKIPPED.swap(0, Ordering::Relaxed);

        //NO VOICE OUTPUT MEANS NOTHING OF OURS
        if self.rate == 0
        {
            return;
        }

        if DESYNC.swap(false, Ordering::Relaxed)
        {
            self.reset(); //A RESET STARTS FROM NOTHING ANYWAY
        } else
        {
            for _ in 0..skipped { self.next_reference(); }
        }

        for frame in chunk.chunks_exact_mut(2)
        {
            let reference = self.next_reference();
            let captured = (frame[0] + frame[1]) * 0.5;

            self.reference.push_back(reference);

            match self.state
            {
                State::Searching =>
                {
                    self.phantoms = 0; //NOTHING IS ALIGNED TO ANYTHING YET
                    self.capture.push_back(captured);

                    while self.reference.len() > consts::AEC_HISTORY { self.reference.pop_front(); }
                    while self.capture.len() > consts::AEC_HISTORY { self.capture.pop_front(); }

                    self.countdown = self.countdown.saturating_sub(1);

                    if self.countdown == 0 && self.capture.len() == consts::AEC_HISTORY
                    {
                        self.search();
                    }
                },

                State::Locked =>
                {
                    if self.phantoms > 0
                    {
                        match self.offset.checked_sub(self.phantoms)
                        {
                            //NO LEAD LEFT TO SLIDE INTO
                            None =>
                            {
                                self.reset();
                                continue;
                            },

                            Some(offset) => self.offset = offset,
                        }

                        self.phantoms = 0;
                    }

                    while self.reference.len() > self.offset + self.weights.len() { self.reference.pop_front(); }

                    //ONE ESTIMATE OFF BOTH CHANNELS, MONO ERROR
                    let estimate = self.estimate();
                    let error = captured - estimate;

                    frame[0] -= estimate;
                    frame[1] -= estimate;

                    let echo = self.gain * self.gain * self.norm / self.norm_taps.max(1) as f32;

                    self.capture_power += (captured * captured - self.capture_power) / self.weights.len() as f32;

                    let confidence = match self.capture_power > 0.
                    {
                        true => (echo / self.capture_power).min(1.),
                        false => 0.,
                    };

                    self.adapt(error, confidence);

                    if confidence >= consts::AEC_ADAPT_RATIO { self.score(captured, error); }
                },
            }
        }
    }

    //THE VOICE OUTPUT STREAM MAY CHANGE UNDER US
    fn follow_output_stream(&mut self)
    {
        let rate = RATE.load(Ordering::Relaxed);

        if rate == self.rate { return; }

        self.rate = rate;
        self.step = if rate == 0 { 0. } else { rate as f32 / consts::SAMPLE_RATE as f32 };

        //A RATE CHANGE MAKES THE RING'S SAMPLES WRONG
        while self.consumer.try_pop().is_some() {}

        self.reset();
    }

    //BACK TO KNOWING NOTHING: PASS THE CAPTURE THROUGH
    fn reset(&mut self)
    {
        self.state = State::Searching;
        self.position = 0.;
        self.current = 0.;
        self.next = 0.;

        self.reference.clear();
        self.capture.clear();

        self.weights.fill(0.);

        self.best.fill(0.);
        self.best_erle = f32::NEG_INFINITY;
        self.failures = 0;

        self.offset = 0;
        self.norm = 0.;
        self.norm_taps = 0;
        self.capture_power = 0.;

        self.countdown = consts::AEC_SEARCH_INTERVAL;
        self.scored = 0;
        self.capture_energy = 0.;
        self.residual_energy = 0.;
        self.phantoms = 0;

    }

    //ONE REFERENCE SAMPLE AT OUR RATE
    fn next_reference(&mut self) -> f32
    {
        while self.position >= 1.
        {
            self.current = self.next;

            self.next = match self.consumer.try_pop()
            {
                Some(sample) => sample,

                None =>
                {
                    self.phantoms += 1;

                    0.
                },
            };

            self.position -= 1.;
        }

        let sample = self.current + (self.next - self.current) * self.position;
        self.position += self.step;

        sample
    }

    //OUR CONTRIBUTION, PLUS THE ENERGY TO NORMALISE BY
    fn estimate(&mut self) -> f32
    {
        let newest = self.reference.len() - 1;
        let mut estimate = 0.;

        self.norm = 0.;
        self.norm_taps = 0;

        for tap in 0..self.weights.len()
        {
            let Some(index) = newest.checked_sub(self.offset + tap) else { break };
            let sample = self.reference[index];

            estimate += self.weights[tap] * sample;
            self.norm += sample * sample;
            self.norm_taps += 1;
        }

        estimate
    }

    //NLMS, THE STEP OVER THE TAP WINDOW'S ENERGY
    fn adapt(&mut self, error: f32, confidence: f32)
    {
        let newest = self.reference.len() - 1;
        let scale = consts::AEC_STEP * confidence * error / (self.norm + consts::AEC_EPSILON);

        for tap in 0..self.weights.len()
        {
            let Some(index) = newest.checked_sub(self.offset + tap) else { break };

            self.weights[tap] += scale * self.reference[index];
        }
    }

    fn score(&mut self, captured: f32, error: f32)
    {
        self.capture_energy += captured * captured;
        self.residual_energy += error * error;
        self.scored += 1;

        if self.scored < consts::AEC_SCORE_WINDOW { return; }

        let lost = self.capture_energy > consts::AEC_SCORE_FLOOR && self.residual_energy > self.capture_energy;

        //WHAT THE FILTER IS ACTUALLY REMOVING
        let erle = match self.residual_energy > 0. && self.capture_energy > 0.
        {
            true => 10. * (self.capture_energy / self.residual_energy).log10(),
            false => 0.,
        };

        self.scored = 0;
        self.capture_energy = 0.;
        self.residual_energy = 0.;

        if self.best_erle.is_finite() { self.best_erle -= consts::AEC_ROLLBACK_DECAY; }

        if !lost && erle > self.best_erle
        {
            self.best.copy_from_slice(&self.weights);
            self.best_erle = erle;
        } else if lost || erle < self.best_erle - consts::AEC_ROLLBACK_MARGIN
        {
            self.weights.copy_from_slice(&self.best);
        }

        match lost
        {
            true => self.failures += 1,
            false => self.failures = 0,
        }

        if self.failures >= consts::AEC_ROLLBACK_LIMIT { self.reset(); }
    }

    fn search(&mut self)
    {
        self.countdown = consts::AEC_SEARCH_INTERVAL;

        let reference: Vec<f32> = self.reference.iter().copied().collect();
        let capture: Vec<f32> = self.capture.iter().copied().collect();

        let window = consts::AEC_WINDOW;
        let captured = &capture[capture.len() - window..];

        //NOTHING IS PLAYING
        if energy(captured) <= 0. || energy(&reference[reference.len() - window..]) < consts::AEC_MIN_ENERGY
        {
            return;
        }

        //COARSE PASS: WHICH LAG, AND WHETHER IT IS A PEAK
        let factor = consts::AEC_SEARCH_DECIMATION;

        let coarse_reference = decimate(&reference, factor);
        let coarse_captured = decimate(captured, factor);

        let coarse_range = (coarse_reference.len().saturating_sub(coarse_captured.len()))
            .min(consts::AEC_SEARCH_RANGE / factor);

        let Some((coarse, _, sigma)) = correlate(&coarse_reference, &coarse_captured, 0..=coarse_range)
        else { return };

        //HOW FAR THE PEAK STANDS ABOVE COINCIDENCE
        if sigma < consts::AEC_PEAK_SIGMA { return; }

        //FINE PASS: THE SAME PEAK AT FULL RATE
        let centre = coarse * factor;
        let lags = centre.saturating_sub(factor)..=(centre + factor).min(consts::AEC_SEARCH_RANGE);

        let Some((delay, _, _)) = correlate(&reference, captured, lags) else { return };

        let start = reference.len() - window - delay;
        let found = &reference[start..start + window];

        let mut correlation = 0.;

        for index in 0..window
        {
            correlation += captured[index] * found[index];
        }

        //LEAST SQUARES FIT OF THE REFERENCE ONTO THE CAPTURE
        let gain = correlation / energy(found);

        if !(consts::AEC_MIN_GAIN..=consts::AEC_MAX_GAIN).contains(&gain) { return; }

        //STRADDLE THE ESTIMATE
        self.offset = delay.saturating_sub(consts::AEC_LEAD_TAPS);

        self.weights.fill(0.);
        self.weights[delay - self.offset] = gain;

        //THE FIT IS THE FILTER TO BEAT AND TO FALL BACK ON
        self.best.copy_from_slice(&self.weights);
        self.best_erle = f32::NEG_INFINITY;
        self.failures = 0;

        self.capture.clear();
        self.capture.shrink_to_fit();

        self.norm = 0.;
        self.scored = 0;
        self.capture_energy = 0.;
        self.residual_energy = 0.;
        self.state = State::Locked;

        self.gain = gain;
    }
}

//FUNCTIONS
fn energy(samples: &[f32]) -> f32
{
    samples.iter().map(|sample| sample * sample).sum()
}

fn decimate(samples: &[f32], factor: usize) -> Vec<f32>
{
    samples[samples.len() % factor..]
        .chunks_exact(factor)
        .map(|chunk| chunk.iter().sum::<f32>() / factor as f32)
        .collect()
}

fn correlate(reference: &[f32], captured: &[f32], lags: RangeInclusive<usize>) -> Option<(usize, f32, f32)>
{
    let window = captured.len();
    let capture_norm = energy(captured).sqrt();

    let (first_lag, last_lag) = (*lags.start(), *lags.end());

    if capture_norm <= 0. || reference.len() < window + last_lag { return None; }

    let mut best = (0usize, f32::NEG_INFINITY);
    let mut total = 0.;
    let mut total_squared = 0.;
    let mut scored = 0.;

    //CARRY THE WINDOW'S ENERGY ACROSS THE LAGS
    let mut first = reference.len() - window - first_lag;
    let mut reference_energy = energy(&reference[first..first + window]);

    for delay in first_lag..=last_lag
    {
        if delay > first_lag
        {
            first -= 1;
            reference_energy += reference[first] * reference[first]
                - reference[first + window] * reference[first + window];
        }

        if reference_energy <= 0. { continue; }

        let mut correlation = 0.;

        for index in 0..window
        {
            correlation += captured[index] * reference[first + index];
        }

        let score = correlation / (reference_energy.sqrt() * capture_norm);

        total += score;
        total_squared += score * score;
        scored += 1.;

        if score > best.1 { best = (delay, score); }
    }

    if scored <= 0. { return None; }

    let mean = total / scored;
    let deviation = (total_squared / scored - mean * mean).max(0.).sqrt();

    //ONE LAG, OR NO SPREAD, IS NO EVIDENCE
    Some(match deviation > 0.
    {
        true => (best.0, best.1, (best.1 - mean) / deviation),
        false => (best.0, best.1, f32::INFINITY),
    })
}

//PUBLIC
//INSTALL THE TAP
pub fn start() -> Option<Canceller>
{
    let (producer, consumer) = HeapRb::<f32>::new(consts::AEC_REFERENCE_CAPACITY).split();

    *REFERENCE.lock().ok()? = Some(producer);

    DESYNC.store(true, Ordering::Relaxed);
    SKIPPED.store(0, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);

    Some(Canceller
    {
        consumer,
        state: State::Searching,

        rate: 0,
        step: 0.,
        position: 0.,
        current: 0.,
        next: 0.,

        reference: VecDeque::with_capacity(consts::AEC_HISTORY + 1),
        capture: VecDeque::with_capacity(consts::AEC_HISTORY + 1),

        weights: vec![0.; consts::AEC_TAPS],
        best: vec![0.; consts::AEC_TAPS],
        best_erle: f32::NEG_INFINITY,
        failures: 0,
        offset: 0,
        norm: 0.,
        norm_taps: 0,
        capture_power: 0.,

        countdown: consts::AEC_SEARCH_INTERVAL,
        scored: 0,
        capture_energy: 0.,
        residual_energy: 0.,

        phantoms: 0,

        gain: 0.,
    })
}

//CALLED WHEN THE CAPTURE DROPS A CHUNK
pub fn skip_reference(frames: usize)
{
    if !ACTIVE.load(Ordering::Relaxed) { return; }

    SKIPPED.fetch_add(frames, Ordering::Relaxed);
}

//UNINSTALL THE TAP
pub fn stop()
{
    ACTIVE.store(false, Ordering::Relaxed);

    if let Ok(mut reference) = REFERENCE.lock()
    {
        *reference = None;
    }
}

//THE REFERENCE RATE, AND A (RE)BUILT STREAM
pub fn set_rate(rate: u32)
{
    RATE.store(rate, Ordering::Relaxed);
    DESYNC.store(true, Ordering::Relaxed);
}

//CALLED FROM THE VOICE OUTPUT CALLBACK
pub fn push_reference(samples: &[f32])
{
    if !ACTIVE.load(Ordering::Relaxed) { return; }

    let Ok(mut reference) = REFERENCE.lock() else { return };
    let Some(reference) = reference.as_mut() else { return };

    //A LOST SAMPLE SHIFTS EVERY LATER ONE
    if reference.push_slice(samples) != samples.len()
    {
        DESYNC.store(true, Ordering::Relaxed);
    }
}
