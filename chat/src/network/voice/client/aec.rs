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
    env,
    collections::VecDeque,
    sync::
    {
        Mutex,
        atomic::{ AtomicBool, AtomicU32, Ordering },
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
        Observer,
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
    history: usize,

    //FILTER
    weights: Vec<f32>,
    offset: usize, //HOW FAR BACK THE FIRST TAP SITS
    norm: f32,     //RUNNING ENERGY OF THE TAP WINDOW

    //GUARDS
    countdown: usize,     //SAMPLES LEFT BEFORE THE NEXT SEARCH IS WORTH ATTEMPTING
    scored: usize,        //SAMPLES IN THE CURRENT ERLE WINDOW
    capture_energy: f32,  //ENERGY THAT WENT INTO IT
    residual_energy: f32, //ENERGY THAT CAME OUT
}

//GLOBAL VARIABLES
static REFERENCE: Mutex<Option<HeapProd<f32>>> = Mutex::new(None); //THE VOICE OUTPUT CALLBACK'S END OF THE TAP
static ACTIVE: AtomicBool = AtomicBool::new(false);                //IS ANYBODY SHARING?
static DESYNC: AtomicBool = AtomicBool::new(false);                //THE REFERENCE LOST SAMPLES - ALIGNMENT IS GONE
static RATE: AtomicU32 = AtomicU32::new(0);                        //SAMPLE RATE OF THE VOICE OUTPUT STREAM

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

        //NO VOICE OUTPUT STREAM MEANS NOTHING OF OURS IS IN THE CAPTURE
        if self.rate == 0
        {
            return;
        }

        //A REFERENCE THAT BACKED UP IS A REFERENCE THAT NO LONGER LINES UP
        if self.consumer.occupied_len() > self.consumer.capacity().get() / 2
        {
            DESYNC.store(true, Ordering::Relaxed);
        }

        if DESYNC.swap(false, Ordering::Relaxed)
        {
            self.reset();
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
                    self.capture.push_back(captured);

                    while self.reference.len() > self.history { self.reference.pop_front(); }
                    while self.capture.len() > self.history { self.capture.pop_front(); }

                    self.countdown = self.countdown.saturating_sub(1);

                    if self.countdown == 0 && self.capture.len() == self.history
                    {
                        self.search();
                    }
                },

                State::Locked =>
                {
                    while self.reference.len() > self.offset + consts::AEC_TAPS { self.reference.pop_front(); }

                    //OUR CONTRIBUTION IS THE SAME IN BOTH CHANNELS (THE VOICE MIX IS MONO ACROSS THEM), SO
                    //ONE ESTIMATE IS SUBTRACTED FROM BOTH AND THE MONO ERROR DRIVES THE ADAPTATION
                    let estimate = self.estimate();
                    let error = captured - estimate;

                    frame[0] -= estimate;
                    frame[1] -= estimate;

                    self.adapt(error);
                    self.score(captured, error);
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

        self.reset();
    }

    //BACK TO KNOWING NOTHING: THE CAPTURE GOES OUT UNTOUCHED UNTIL THE DELAY IS FOUND AGAIN
    fn reset(&mut self)
    {
        while self.consumer.try_pop().is_some() {}

        self.state = State::Searching;
        self.position = 0.;
        self.current = 0.;
        self.next = 0.;

        self.reference.clear();
        self.capture.clear();

        self.weights.fill(0.);
        self.norm = 0.;

        self.countdown = consts::AEC_SEARCH_INTERVAL;
        self.scored = 0;
        self.capture_energy = 0.;
        self.residual_energy = 0.;
    }

    //ONE REFERENCE SAMPLE AT OUR RATE. AN EMPTY RING READS AS SILENCE, WHICH IS EXACTLY RIGHT - THERE IS
    //NOTHING OF OURS TO TAKE OUT OF THE CAPTURE WHILE WE ARE NOT PLAYING ANYTHING.
    fn next_reference(&mut self) -> f32
    {
        while self.position >= 1.
        {
            self.current = self.next;
            self.next = self.consumer.try_pop().unwrap_or(0.);
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

        for tap in 0..consts::AEC_TAPS
        {
            let Some(index) = newest.checked_sub(self.offset + tap) else { break };
            let sample = self.reference[index];

            estimate += self.weights[tap] * sample;
            self.norm += sample * sample;
        }

        estimate
    }

    //NLMS. THE STEP IS DIVIDED BY THE ENERGY IN THE TAP WINDOW (SUMMED BY `estimate`, WHICH HAS ALREADY
    //WALKED IT), SO THE FILTER MOVES AT THE SAME PACE WHETHER THE CHANNEL IS LOUD OR QUIET, AND NOT AT ALL
    //WHILE IT IS SILENT. THE STEP ITSELF IS TINY ON PURPOSE - SEE consts::AEC_STEP.
    fn adapt(&mut self, error: f32)
    {
        let newest = self.reference.len() - 1;
        let scale = consts::AEC_STEP * error / (self.norm + consts::AEC_EPSILON);

        for tap in 0..consts::AEC_TAPS
        {
            let Some(index) = newest.checked_sub(self.offset + tap) else { break };

            self.weights[tap] += scale * self.reference[index];
        }
    }

    //A FILTER THAT IS ADDING ENERGY INSTEAD OF REMOVING IT HAS LOST THE ALIGNMENT (THE USER MOVED THE
    //VOLUME SLIDER, THE TWO DEVICES DRIFTED APART) - GIVE UP THE LOCK RATHER THAN KEEP DAMAGING THE SHARE
    fn score(&mut self, captured: f32, error: f32)
    {
        self.capture_energy += captured * captured;
        self.residual_energy += error * error;
        self.scored += 1;

        if self.scored < consts::AEC_SCORE_WINDOW { return; }

        let lost = self.capture_energy > consts::AEC_SCORE_FLOOR && self.residual_energy > self.capture_energy;

        self.scored = 0;
        self.capture_energy = 0.;
        self.residual_energy = 0.;

        if lost
        {
            self.reset();
        }
    }

    //FINDS HOW FAR BEHIND THE CAPTURE OUR OWN PLAYBACK IS, BY CORRELATING THE TWO AT EVERY LAG IN RANGE.
    //
    //WHAT IT MUST NOT DO IS DEMAND A STRONG CORRELATION. WHATEVER IS BEING SHARED IS IN THE CAPTURE TOO,
    //AND IT IS OFTEN THE LOUDER HALF BY FAR - A VIDEO PLAYING OVER A QUIET VOICE CHANNEL DRAGS THE
    //CORRELATION AT THE *CORRECT* LAG DOWN TO 0.1 OR BELOW, SO ANY FIXED FLOOR EITHER REJECTS THE RIGHT
    //ANSWER OR ACCEPTS EVERY WRONG ONE. WHAT SEPARATES THEM IS NOT THE PEAK'S HEIGHT BUT HOW FAR IT STANDS
    //ABOVE THE OTHER LAGS: THOSE ARE UNCORRELATED, SO THEY SCATTER AROUND ZERO WITH A KNOWN SPREAD, AND A
    //REAL ECHO CLEARS IT BY SIGMAS EVEN WHEN IT IS BURIED UNDER THE SHARE.
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

        self.capture.clear();
        self.capture.shrink_to_fit();

        self.norm = 0.;
        self.scored = 0;
        self.capture_energy = 0.;
        self.residual_energy = 0.;
        self.state = State::Locked;
    }
}

//FUNCTIONS
fn energy(samples: &[f32]) -> f32
{
    samples.iter().map(|sample| sample * sample).sum()
}


//PUBLIC
//IS THE CANCELLER WANTED AT ALL?
pub fn enabled() -> bool
{
    !env::var(consts::AEC_OVERRIDE_VAR).unwrap_or_default().eq_ignore_ascii_case("off")
}

//INSTALLS THE TAP. THE SCREEN CAPTURE CALLS THIS ONCE, AND DROPS THE CANCELLER WHEN THE SHARE ENDS.
pub fn start() -> Option<Canceller>
{
    if !enabled() { return None; }

    let (producer, consumer) = HeapRb::<f32>::new(consts::AEC_REFERENCE_CAPACITY).split();

    *REFERENCE.lock().ok()? = Some(producer);

    DESYNC.store(true, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);

    let history = consts::AEC_SEARCH_RANGE + consts::AEC_WINDOW;

    Some(Canceller
    {
        consumer,
        state: State::Searching,

        rate: 0,
        step: 0.,
        position: 0.,
        current: 0.,
        next: 0.,

        reference: VecDeque::with_capacity(history + 1),
        capture: VecDeque::with_capacity(history + 1),
        history,

        weights: vec![0.; consts::AEC_TAPS],
        offset: 0,
        norm: 0.,

        countdown: consts::AEC_SEARCH_INTERVAL,
        scored: 0,
        capture_energy: 0.,
        residual_energy: 0.,
    })
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

#[cfg(test)]
#[cfg(test)]
mod test
{
    use super::*;

    //THE TAP IS A GLOBAL - TWO TESTS RUNNING AT ONCE WOULD BE ONE TEST WATCHING THE OTHER'S REFERENCE
    static SERIAL: Mutex<()> = Mutex::new(());

    const DELAY: usize = 3000; //HOW FAR THE SINK IS BEHIND US (~62ms)
    const GAIN: f32 = 0.7;     //WHAT THE PER-STREAM VOLUME DID TO IT ON THE WAY
    const SECONDS: usize = 8;
    const CHUNK: usize = 960;

    struct Noise(u32); //DETERMINISTIC, SO A FAILURE IS ALWAYS THE SAME FAILURE

    impl Noise
    {
        fn next(&mut self) -> f32
        {
            self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);

            (self.0 >> 8) as f32 / (1 << 23) as f32 - 1.
        }
    }

    //SPEECH-SHAPED: BURSTS WITH GAPS AND CHANGING LOUDNESS. A FLAT SIGNAL WOULD HAVE NO ENVELOPE AND
    //NOTHING IN A VOICE CHANNEL LOOKS LIKE THAT.
    fn talking(seed: u32, samples: usize) -> Vec<f32>
    {
        let mut noise = Noise(seed);
        let mut pattern = Noise(seed.wrapping_mul(2654435761));
        let mut signal = Vec::with_capacity(samples);
        let mut level = 0.;

        for index in 0..samples
        {
            //A NEW BURST EVERY ~250ms, LOUD OR SILENT ON ITS OWN COIN - TWO PEOPLE TALKING DO NOT SHARE AN
            //ENVELOPE, AND A TEST WHERE THEY DID WOULD BE TESTING THE WRONG THING
            if index % (consts::SAMPLE_RATE as usize / 4) == 0
            {
                let coin = pattern.next();

                level = if coin < -0.2 { 0. } else { 0.15 + 0.25 * (coin + 1.) * 0.5 };
            }

            signal.push(noise.next() * level);
        }

        signal
    }

    //THE SHARED AUDIO ITSELF. UNLIKE SPEECH IT DOES NOT STOP, WHICH IS PRECISELY WHAT MAKES IT HARD - IT
    //SITS ON TOP OF THE ECHO THE WHOLE TIME INSTEAD OF LEAVING GAPS TO FIND IT IN.
    fn playing(seed: u32, samples: usize, level: f32) -> Vec<f32>
    {
        let mut noise = Noise(seed);

        (0..samples).map(|_| noise.next() * level).collect()
    }

    //RUNS A WHOLE SHARE PAST THE CANCELLER AND REPORTS HOW MUCH OF OUR OWN PLAYBACK IT TOOK BACK OUT (dB),
    //ALONGSIDE HOW MUCH OF THE SHARED AUDIO IT DAMAGED DOING SO
    fn share(content: &[f32]) -> (f32, f32)
    {
        let samples = consts::SAMPLE_RATE as usize * SECONDS;
        let reference = talking(1, samples);                       //THE VOICE CHANNEL, AS THE SINK GOT IT
        let measured = samples - consts::SAMPLE_RATE as usize * 3; //FIRST SAMPLE THAT COUNTS (PAST THE SEARCH)

        set_rate(consts::SAMPLE_RATE);

        let mut canceller = start().expect("the canceller is enabled by default");

        let mut echo_before = 0.;
        let mut echo_after = 0.;
        let mut content_energy = 0.;

        let echo_at = |index: usize| if index >= DELAY { reference[index - DELAY] * GAIN } else { 0. };

        for start in (0..samples).step_by(CHUNK)
        {
            let end = (start + CHUNK).min(samples);

            push_reference(&reference[start..end]);

            //THE MONITOR: WHAT WE ARE SHARING, PLUS OUR OWN PLAYBACK DELAYED AND RESCALED BY THE SINK
            let mut chunk = Vec::with_capacity((end - start) * 2);

            for index in start..end
            {
                chunk.push(content[index] + echo_at(index));
                chunk.push(content[index] + echo_at(index));
            }

            canceller.process(&mut chunk);

            for (offset, frame) in chunk.chunks_exact(2).enumerate()
            {
                let index = start + offset;

                if index < measured { continue; }

                //BOTH CHANNELS CARRY THE SAME ESTIMATE, SO THE SUBTRACTION CANNOT PULL THE IMAGE APART
                assert_eq!(frame[0], frame[1]);

                let residual = frame[0] - content[index];

                echo_before += echo_at(index) * echo_at(index);
                echo_after += residual * residual;
                content_energy += content[index] * content[index];
            }
        }

        let removed = 10. * (echo_before / echo_after.max(f32::MIN_POSITIVE)).log10();
        let damage = 10. * (echo_after / content_energy.max(f32::MIN_POSITIVE)).log10();

        (removed, damage)
    }

    //THE WHOLE POINT: WHAT WE PLAY OUT COMES BACK IN THE MONITOR, AND HAS TO LEAVE AGAIN BEFORE THE SHARE
    //IS ENCODED - WITHOUT TAKING THE AUDIO WE ARE ACTUALLY SHARING WITH IT
    #[test]
    fn our_own_playback_is_taken_back_out_of_the_capture()
    {
        let _serial = SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

        let content = talking(7, consts::SAMPLE_RATE as usize * SECONDS);
        let (removed, damage) = share(&content);

        assert!(removed > 25., "only {removed:.1} dB of our own playback was removed");
        assert!(damage < -30., "the shared audio was damaged ({damage:.1} dB of it is residual)");
    }

    //A VIDEO PLAYING WHILE WE SHARE. THIS IS THE CASE A FIXED CORRELATION FLOOR GETS WRONG: THE SHARE IS
    //FOUR TIMES THE ECHO AND NEVER PAUSES, SO THE CORRELATION AT THE CORRECT LAG IS WEAK IN ABSOLUTE TERMS
    //AND THE DELAY IS ONLY FINDABLE BY HOW FAR IT STANDS OUT FROM THE LAGS AROUND IT.
    #[test]
    fn a_loud_share_does_not_hide_the_echo()
    {
        let _serial = SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

        let content = playing(7, consts::SAMPLE_RATE as usize * SECONDS, 0.4);
        let (removed, damage) = share(&content);

        assert!(removed > 25., "only {removed:.1} dB of our own playback was removed");
        assert!(damage < -30., "the shared audio was damaged ({damage:.1} dB of it is residual)");
    }

    //NOBODY IS IN A VOICE CHANNEL: THERE IS NOTHING OF OURS IN THE MONITOR, SO THE CAPTURE IS NOT TOUCHED
    #[test]
    fn a_silent_reference_changes_nothing()
    {
        let _serial = SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

        set_rate(0);

        let mut canceller = start().expect("the canceller is enabled by default");
        let mut chunk: Vec<f32> = (0..CHUNK * 2).map(|index| (index as f32 * 0.01).sin()).collect();
        let original = chunk.clone();

        canceller.process(&mut chunk);

        assert_eq!(chunk, original);
    }
}
