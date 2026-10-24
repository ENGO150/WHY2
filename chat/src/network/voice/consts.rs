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

//! # Voice Constants
//!
//! Codec framing, the capture processing chain (AGC, limiter, noise gate), the jitter buffer,
//! voice message limits and the share-audio echo canceller's tuning.
//!
//! Most of these are tied to the codec or to each other; change them only together with
//! whatever depends on them.

//CONSTS (I HIGHLY RECOMMEND NOT CHANGING THOSE)
/// Sample rate of every stream, in Hz.
pub const SAMPLE_RATE: u32          = 48000;                                    //put some text here
/// Length of one Opus frame, in milliseconds.
pub const FRAME_MS: u32             = 20;                                       //LENGTH OF ONE FRAME
/// Samples per channel in one frame (960).
pub const FRAME_SIZE: usize         = (SAMPLE_RATE * FRAME_MS / 1000) as usize; //960 SAMPLES PER FRAME

/// Target RMS after automatic gain control (about -18 dBFS).
pub const AGC_TARGET_RMS: f32       = 0.12;                                     //TARGET RMS AFTER NORMALIZATION (~-18dBFS)
/// AGC envelope attack coefficient (about 80 ms).
pub const AGC_ATTACK: f32           = 0.25;                                     //ENVELOPE RISE (~80ms)
/// AGC envelope release coefficient (about 1 s).
pub const AGC_RELEASE: f32          = 0.02;                                     //ENVELOPE FALL (~1s)
/// AGC gain slew while boosting (about 1 s; slow, so no pumping).
pub const AGC_GAIN_UP: f32          = 0.02;                                     //GAIN SLEW WHILE BOOSTING (~1s, SLOW = NO PUMPING)
/// AGC gain slew while cutting (about 65 ms; fast, so no clipping).
pub const AGC_GAIN_DOWN: f32        = 0.3;                                      //GAIN SLEW WHILE CUTTING (~65ms, FAST = NO CLIPPING)
/// Largest AGC gain (+32 dB).
pub const AGC_MAX_GAIN: f32         = 40.0;                                     //MAX GAIN (+32dB, RESCUES QUIET MICS)
/// Smallest AGC gain (-26 dB).
pub const AGC_MIN_GAIN: f32         = 0.05;                                     //MIN GAIN (-26dB, TAMES HOT MICS)
/// Amplitude the soft limiter starts at.
pub const LIMITER_KNEE: f32         = 0.7;                                      //SOFT CLIPPING STARTS AT THIS AMPLITUDE

/// Noise floor EMA coefficient while the floor falls.
pub const NOISE_FLOOR_ALPHA: f32    = 0.05;                                     //EMA SMOOTHING FACTOR WHILE THE FLOOR FALLS
/// Noise floor EMA coefficient while it rises; much slower, so speech cannot teach the gate it
/// is noise.
pub const NOISE_FLOOR_RISE: f32     = 0.005;                                    //EMA SMOOTHING FACTOR WHILE IT RISES
/// Gate opens at this multiple of the noise floor.
pub const NOISE_OPEN_MULT: f32      = 3.5;                                      //OPEN TRESHOLD
/// Gate closes at this multiple of the noise floor (hysteresis).
pub const NOISE_CLOSE_MULT: f32     = 2.0;                                      //CLOSE TRESHOLD (HYSTERESIS)
/// Lowest opening threshold.
pub const MIN_TRESHOLD_OPEN: f32    = 0.0008;                                   //HARD MINIMUM FOR OPEN
/// Lowest closing threshold.
pub const MIN_TRESHOLD_CLOSE: f32   = 0.0004;                                   //HARD MINIMUM FOR CLOSE
/// Noise floor a capture starts from.
pub const INITIAL_NOISE_FLOOR: f32  = 0.003;                                    //INIT

/// Frames the gate stays open after speech stops (about 200 ms).
pub const HOLD_FRAMES: usize        = 10;                                       //~200ms HOLD TIME
/// Level above which a received stream counts as speaking.
pub const MIXING_TRESHOLD: f32      = 0.001;                                    //SPEAKER DETECTION NOISE TRESHOLD
/// Frames between activity reports to the server (about 2 s).
pub const ACTIVITY_TRESHOLD: usize  = 100;                                      //SERVER ACTIVITY TIMER RESET TRESHOLD (~2000ms)
/// Volume of the join and leave sound effects.
pub const SOUND_EFFECT_VOLUME: f32  = 0.1;                                      //VOLUME OF SOUND EFFECTS (10%)

/// Samples a speaker stays marked active after they stop (about 100 ms).
pub const ACTIVITY_HOLD: usize      = (SAMPLE_RATE / 10) as usize;              //HOW LONG AFTER SPEAKING CLIENT BECOMES INACTIVE (~100ms)
/// Milliseconds the voice panel keeps a speaker highlighted.
pub const DISPLAY_HOLD: usize       = 200;                                      //ACTIVITY_HOLD BUT MS FOR DISPLAY WINDOW

/// Frames each peer's jitter buffer holds.
pub const JITTER_BUFFER_SIZE: usize = 20;                                       //FRAME SIZE OF JITTER BUFFER
/// Jitter buffer depth the catch-up stops at (about 60 ms).
pub const JITTER_TARGET: usize      = FRAME_SIZE * 3;                           //DEPTH THE CATCH-UP STOPS AT (~60ms)
/// Jitter buffer depth that is shed back to the target (about 200 ms).
pub const JITTER_MAX: usize         = FRAME_SIZE * 10;                          //DEPTH SHED BACK TO TARGET (~200ms)
/// Playback speed-up while the jitter buffer is above target.
pub const JITTER_CATCHUP: f32       = 0.02;                                     //PLAYBACK SPEEDUP WHILE ABOVE TARGET
/// Preferred audio device period (about 20 ms).
pub const DEVICE_BUFFER: u32        = FRAME_SIZE as u32;                        //PREFERRED DEVICE PERIOD (~20ms)

/// Grid width voice packets are encrypted with.
pub const GRID_WIDTH: usize         = 4;                                        //GRID WIDTH FOR VOICE PACKETS
/// Grid height voice packets are encrypted with.
pub const GRID_HEIGHT: usize        = 4;                                        //GRID HEIGHT FOR VOICE PACKETS

/// Gap between two `Hello` attempts, in milliseconds.
pub const HELLO_INTERVAL: u64       = 200;                                      //GAP BETWEEN TWO Hello ATTEMPTS (MS)
/// How long the `Hello` handshake may go unanswered, in milliseconds.
pub const HELLO_TIMEOUT: u64        = 5000;                                     //HOW LONG THE HANDSHAKE MAY GO UNANSWERED (MS)

/// Client UDP receive poll timeout, in milliseconds.
pub const RECV_TIMEOUT: u64         = 200;                                      //UDP RECEIVE POLL TIMEOUT (MS)

/// Packets per second one voice session may sustain: the frame rate plus headroom for jitter,
/// $1.5 \times 1000 / 20 = 75$.
pub const MAX_PACKET_RATE: f32      = (1000 / FRAME_MS) as f32 * 1.5;           //PACKETS PER SECOND ONE VOICE SESSION MAY SUSTAIN
/// Packets a voice session may send at once (about 500 ms of jitter and reordering).
pub const MAX_PACKET_BURST: f32     = MAX_PACKET_RATE / 2.0;                    //WHAT IT MAY SEND AT ONCE (~500ms OF JITTER AND REORDERING)
/// Capacity of the audio callback to network task channel.
pub const SEND_CHANNEL_BOUND: usize = 8;                                        //AUDIO CALLBACK -> NETWORK TASK BUFFER
/// Loudest volume setting, in percent.
pub const VOLUME_MAX: u32           = 200;                                      //LOUDEST VOLUME SETTING (PERCENT)

/// Opus bitrate of voice messages.
pub const MESSAGE_BITRATE: i32      = 96_000;                                   //VOICE MESSAGE OPUS BITRATE
/// Channels voice messages are recorded in.
pub const MESSAGE_CHANNELS: u8      = 2;                                        //VOICE MESSAGES ARE RECORDED IN STEREO
/// Longest voice message, in frames (5 minutes).
pub const MESSAGE_MAX_FRAMES: usize = (300 * 1000 / FRAME_MS) as usize;         //LONGEST VOICE MESSAGE (5 MINUTES)
/// Shortest voice message worth sending, in frames (300 ms).
pub const MESSAGE_MIN_FRAMES: usize = (300 / FRAME_MS) as usize;                //SHORTEST ONE WORTH SENDING (300ms)
/// Opus's ceiling on the size of one frame's packet.
pub const MESSAGE_MAX_PACKET: usize = 1275;                                     //BIGGEST OPUS PACKET OF ONE FRAME

/// Capacity of the echo canceller's reference ring (2 s).
pub const AEC_REFERENCE_CAPACITY: usize   = (SAMPLE_RATE * 2) as usize;         //REFERENCE RING
/// Furthest the playback may lag the capture, in samples.
pub const AEC_SEARCH_RANGE: usize         = (SAMPLE_RATE * 3 / 10) as usize;    //FURTHEST THE PLAYBACK MAY LAG THE CAPTURE
/// Samples each candidate lag is scored over (about 85 ms).
pub const AEC_WINDOW: usize               = 4096;                               //SAMPLES EACH LAG IS SCORED OVER (~85ms)
/// Minimum gap between two delay searches (about 100 ms).
pub const AEC_SEARCH_INTERVAL: usize      = (SAMPLE_RATE / 10) as usize;        //MINIMUM GAP BETWEEN TWO SEARCHES (~100ms)
/// Decimation factor of the coarse delay search.
pub const AEC_SEARCH_DECIMATION: usize    = 4;                                  //HOW FAR THE SEARCH DECIMATES
/// How many standard deviations a correlation peak must stand above the other lags.
pub const AEC_PEAK_SIGMA: f32             = 6.0;                                //HOW FAR THE PEAK MUST STAND ABOVE THE REST
/// Reference energy below this is silence, not a signal.
pub const AEC_MIN_ENERGY: f32             = 0.05;                               //REFERENCE ENERGY BELOW THIS IS SILENCE, NOT A SIGNAL
/// Fraction of the capture that must be our echo for the filter to adapt at full step.
pub const AEC_ADAPT_RATIO: f32            = 0.5;                                //HOW MUCH OF THE CAPTURE MUST BE OUR ECHO
/// Quietest plausible echo gain (-26 dB).
pub const AEC_MIN_GAIN: f32               = 0.05;                               //QUIETEST PLAUSIBLE ECHO (-26dB)
/// Loudest plausible echo gain (+6 dB).
pub const AEC_MAX_GAIN: f32               = 2.0;                                //LOUDEST PLAUSIBLE ECHO (+6dB)
/// Adaptive filter length (about 5 ms around the estimate).
pub const AEC_TAPS: usize                 = 256;                                //FILTER LENGTH (~5ms AROUND THE ESTIMATE)
/// Taps of the filter ahead of the estimated delay (about 1 ms).
pub const AEC_LEAD_TAPS: usize            = 64;                                 //HOW MUCH OF IT SITS AHEAD OF THE ESTIMATE (~1ms)
/// NLMS step size.
pub const AEC_STEP: f32                   = 0.0001;                              //NLMS STEP SIZE
/// NLMS regularisation, so a silent window is never divided by.
pub const AEC_EPSILON: f32                = 1e-6;                               //NLMS REGULARIZATION (NEVER DIVIDE BY A SILENT WINDOW)
/// Samples between two scorings of the filter (about 1 s).
pub const AEC_SCORE_WINDOW: usize         = SAMPLE_RATE as usize;               //HOW OFTEN THE FILTER HAS TO JUSTIFY ITSELF (~1s)
/// dB worse than the best score before the filter is rolled back.
pub const AEC_ROLLBACK_MARGIN: f32        = 3.0;                                //dB WORSE THAN THE BEST BEFORE A ROLLBACK
/// dB the best score forgets each window.
pub const AEC_ROLLBACK_DECAY: f32         = 1.0;                                //dB THE STANDARD TO BEAT FORGETS EACH WINDOW
/// Energy-adding windows in a row before the delay itself is given up.
pub const AEC_ROLLBACK_LIMIT: usize       = 3;                                  //ROLLBACKS IN A ROW THAT DO NOT RESCUE IT
/// Energy below which a window is too quiet to score.
pub const AEC_SCORE_FLOOR: f32            = 0.05;                               //TOO LITTLE WENT THROUGH THAT WINDOW TO JUDGE IT
/// Samples of each side the delay search needs.
pub const AEC_HISTORY: usize              = AEC_SEARCH_RANGE + AEC_WINDOW;      //HOW MUCH OF EACH SIDE THE SEARCH NEEDS
