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
    Searching, //THE DELAY IS NOT KNOWN YET - COLLECTING HISTORY, PASSING THE CAPTURE THROUGH UNTOUCHED
    Locked,    //THE FILTER IS RUNNING
}

//THE SCREEN CAPTURE'S END OF THE TAP. DROPPING IT UNINSTALLS THE TAP, SO EVERY EARLY RETURN IN THE CAPTURE
//TASK CLEANS UP BY ITSELF.
pub struct Canceller
{
    consumer: HeapCons<f32>,
    state: State,

    //REFERENCE RESAMPLER (THE VOICE OUTPUT DEVICE'S RATE -> OURS). RATE 0 MEANS NO VOICE OUTPUT STREAM YET.
    rate: u32,
    step: f32,
    position: f32,
    current: f32,
    next: f32,

    //HISTORY, NEWEST AT THE BACK. `reference` IS ALSO THE FILTER'S DELAY LINE ONCE WE ARE LOCKED.
    reference: VecDeque<f32>,
    capture: VecDeque<f32>,

    //FILTER
    weights: Vec<f32>,
    best: Vec<f32>,   //THE BEST FILTER THIS LOCK HAS MANAGED, TO GO BACK TO WHEN ADAPTATION MAKES THINGS WORSE
    best_erle: f32,   //WHAT IT SCORED
    failures: usize,  //SCORING WINDOWS PUTTING IT BACK HAS NOT RESCUED
    offset: usize,      //HOW FAR BACK THE FIRST TAP SITS
    norm: f32,          //ENERGY OF THE TAP WINDOW
    norm_taps: usize,   //HOW MANY TAPS THAT WAS SUMMED OVER - FEWER THAN THE FILTER UNTIL THE LINE FILLS
    capture_power: f32, //MEAN SQUARE OF THE CAPTURE, OVER THE SPAN THE TAP WINDOW COVERS

    //GUARDS
    countdown: usize,     //SAMPLES LEFT BEFORE THE NEXT SEARCH IS WORTH ATTEMPTING
    scored: usize,        //SAMPLES IN THE CURRENT ERLE WINDOW
    capture_energy: f32,  //ENERGY THAT WENT INTO IT
    residual_energy: f32, //ENERGY THAT CAME OUT

    //REFERENCE SAMPLES THE RING COULD NOT SUPPLY, WHICH WENT INTO THE DELAY LINE AS SILENCE ANYWAY
    phantoms: usize,

    //WHAT THE SEARCH FOUND, WHICH THE ECHO ESTIMATE STILL NEEDS
    gain: f32,
}

//HOW MUCH OF EACH SIDE THE SEARCH HAS TO HAVE IN HAND BEFORE IT CAN RUN: A FULL WINDOW AT EVERY LAG IN RANGE
const HISTORY: usize = consts::AEC_SEARCH_RANGE + consts::AEC_WINDOW;

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
    //ONE CAPTURED CHUNK, STEREO INTERLEAVED AT consts::SAMPLE_RATE, CANCELLED IN PLACE
    pub fn process(&mut self, chunk: &mut [f32])
    {
        self.follow_output_stream();

        //CAPTURED FRAMES THE CHANNEL COULD NOT HOLD, WHICH THEREFORE NEVER REACHED US
        let skipped = SKIPPED.swap(0, Ordering::Relaxed);

        //NO VOICE OUTPUT STREAM MEANS NOTHING OF OURS IS IN THE CAPTURE
        if self.rate == 0
        {
            return;
        }

        if DESYNC.swap(false, Ordering::Relaxed)
        {
            self.reset(); //A RESET STARTS FROM NOTHING ANYWAY, SO THE SKIP HAS NOTHING LEFT TO CORRECT
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

                    while self.reference.len() > HISTORY { self.reference.pop_front(); }
                    while self.capture.len() > HISTORY { self.capture.pop_front(); }

                    self.countdown = self.countdown.saturating_sub(1);

                    if self.countdown == 0 && self.capture.len() == HISTORY
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
                            //RUN OUT OF LEAD AND THERE IS NOTHING LEFT TO SLIDE INTO - THE MATCHING
                            //REFERENCE WOULD BE NEWER THAN ANYTHING WE HAVE CONSUMED
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

                    //OUR CONTRIBUTION IS THE SAME IN BOTH CHANNELS (THE VOICE MIX IS MONO ACROSS THEM), SO
                    //ONE ESTIMATE IS SUBTRACTED FROM BOTH AND THE MONO ERROR DRIVES THE ADAPTATION
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

    //THE VOICE OUTPUT STREAM CAN START, STOP OR BE REBUILT ONTO ANOTHER DEVICE UNDER US
    fn follow_output_stream(&mut self)
    {
        let rate = RATE.load(Ordering::Relaxed);

        if rate == self.rate { return; }

        self.rate = rate;
        self.step = if rate == 0 { 0. } else { rate as f32 / consts::SAMPLE_RATE as f32 };

        //A CHANGE OF RATE IS THE ONE THING THAT MAKES THE SAMPLES ALREADY IN THE RING WRONG RATHER THAN
        //MERELY UNALIGNED - THEY WERE WRITTEN BY A STREAM THAT NO LONGER EXISTS, AT ANOTHER DEVICE'S RATE
        while self.consumer.try_pop().is_some() {}

        self.reset();
    }

    //BACK TO KNOWING NOTHING: THE CAPTURE GOES OUT UNTOUCHED UNTIL THE DELAY IS FOUND AGAIN
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

    //ONE REFERENCE SAMPLE AT OUR RATE. AN EMPTY RING READS AS SILENCE, WHICH IS EXACTLY RIGHT - THERE IS
    //NOTHING OF OURS TO TAKE OUT OF THE CAPTURE WHILE WE ARE NOT PLAYING ANYTHING.
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

    //WHAT OUR OWN PLAYBACK IS CONTRIBUTING TO THIS SAMPLE, PLUS THE ENERGY THE ADAPTATION NORMALISES BY
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

    //NLMS. THE STEP IS DIVIDED BY THE ENERGY IN THE TAP WINDOW (SUMMED BY `estimate`, WHICH HAS ALREADY
    //WALKED IT), SO THE FILTER MOVES AT THE SAME PACE WHETHER THE CHANNEL IS LOUD OR QUIET, AND NOT AT ALL
    //WHILE IT IS SILENT. THE STEP ITSELF IS TINY ON PURPOSE - SEE consts::AEC_STEP.
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

        //WHAT THE FILTER IS ACTUALLY REMOVING, WHICH IS THE ONE NUMBER WORTH LOOKING AT WHILE TUNING IT
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
        let capture_norm = energy(captured).sqrt();

        //NOTHING IS PLAYING - THERE IS NOTHING TO LINE UP AGAINST YET
        if capture_norm <= 0. || energy(&reference[reference.len() - window..]) < consts::AEC_MIN_ENERGY
        {
            return;
        }

        let mut best = (0usize, f32::NEG_INFINITY);
        let mut total = 0.;
        let mut total_squared = 0.;

        //THE REFERENCE WINDOW SLIDES ONE SAMPLE PER LAG, SO ITS ENERGY IS CARRIED ACROSS INSTEAD OF RESUMMED
        let mut first = reference.len() - window;
        let mut reference_energy = energy(&reference[first..]);

        for delay in 0..=consts::AEC_SEARCH_RANGE
        {
            if delay > 0
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

            if score > best.1 { best = (delay, score); }
        }

        //HOW FAR THE PEAK STANDS ABOVE THE LAGS THAT ARE ONLY COINCIDENCE
        let lags = (consts::AEC_SEARCH_RANGE + 1) as f32;
        let mean = total / lags;
        let deviation = (total_squared / lags - mean * mean).max(0.).sqrt();

        if best.1 < mean + consts::AEC_PEAK_SIGMA * deviation { return; }

        let delay = best.0;
        let start = reference.len() - window - delay;
        let found = &reference[start..start + window];

        let mut correlation = 0.;

        for index in 0..window
        {
            correlation += captured[index] * found[index];
        }

        //LEAST SQUARES FIT OF THE REFERENCE ONTO THE CAPTURE - WHERE THE FILTER STARTS FROM, RATHER THAN
        //FROM NOTHING. A GAIN THIS FAR FROM UNITY IS NOT OUR OWN AUDIO COMING BACK BUT A COINCIDENCE IN
        //SOMEBODY ELSE'S, AND SUBTRACTING IT WOULD EAT WHAT WE ARE MEANT TO BE SHARING.
        let gain = correlation / energy(found);

        if !(consts::AEC_MIN_GAIN..=consts::AEC_MAX_GAIN).contains(&gain) { return; }

        //STRADDLE THE ESTIMATE, SO THE FILTER CAN CORRECT IN EITHER DIRECTION
        self.offset = delay.saturating_sub(consts::AEC_LEAD_TAPS);

        self.weights.fill(0.);
        self.weights[delay - self.offset] = gain;

        //THE LEAST SQUARES FIT IS THE FILTER TO BEAT, AND THE ONE TO FALL BACK ON UNTIL SOMETHING BEATS IT
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

//PUBLIC
//INSTALLS THE TAP. THE SCREEN CAPTURE CALLS THIS ONCE, AND DROPS THE CANCELLER WHEN THE SHARE ENDS.
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

        reference: VecDeque::with_capacity(HISTORY + 1),
        capture: VecDeque::with_capacity(HISTORY + 1),

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

//CALLED FROM THE SCREEN CAPTURE CALLBACK WHEN A CHUNK IS DROPPED ON THE FLOOR - SEE process()
pub fn skip_reference(frames: usize)
{
    if !ACTIVE.load(Ordering::Relaxed) { return; }

    SKIPPED.fetch_add(frames, Ordering::Relaxed);
}

//UNINSTALLS THE TAP. THE VOICE OUTPUT CALLBACK IS BACK TO A SINGLE ATOMIC LOAD PER CALLBACK.
pub fn stop()
{
    ACTIVE.store(false, Ordering::Relaxed);

    if let Ok(mut reference) = REFERENCE.lock()
    {
        *reference = None;
    }
}

//THE RATE THE VOICE OUTPUT CALLBACK PRODUCES THE REFERENCE AT. ALSO THE SIGNAL THAT THE STREAM WAS
//(RE)BUILT, WHICH INVALIDATES ANY ALIGNMENT WE HAD.
pub fn set_rate(rate: u32)
{
    RATE.store(rate, Ordering::Relaxed);
    DESYNC.store(true, Ordering::Relaxed);
}

//CALLED FROM THE VOICE OUTPUT CALLBACK WITH ONE SAMPLE PER FRAME, AFTER EVERYTHING THAT SHAPES IT
pub fn push_reference(samples: &[f32])
{
    if !ACTIVE.load(Ordering::Relaxed) { return; }

    let Ok(mut reference) = REFERENCE.lock() else { return };
    let Some(reference) = reference.as_mut() else { return };

    //DROPPED REFERENCE SAMPLES ARE NOT A GLITCH WE CAN RIDE OUT - EVERY LATER SAMPLE WOULD BE OFF BY
    //HOWEVER MANY WENT MISSING, SO THE DELAY HAS TO BE FOUND AGAIN
    if reference.push_slice(samples) != samples.len()
    {
        DESYNC.store(true, Ordering::Relaxed);
    }
}
